package app.larmamic

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import android.os.VibrationEffect
import android.os.Vibrator
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.media.VolumeProviderCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch

/**
 * Foreground service: keeps the microphone and the connection alive when the screen is off or the app is in the
 * background. Android requires the ongoing notification while the microphone is in use.
 */
class MicService : Service() {
    private var cpu: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null
    private var session: MediaSessionCompat? = null
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    override fun onCreate() {
        super.onCreate()
        Engine.init(this)
        val ch = NotificationChannel(CHANNEL, getString(R.string.notif_title), NotificationManager.IMPORTANCE_LOW)
        (getSystemService(NOTIFICATION_SERVICE) as NotificationManager).createNotificationChannel(ch)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                Engine.stop()
                stopSelf()
            }
            ACTION_TOGGLE_MUTE -> toggleMute()
            else -> {
                val conn = Store(this).connection
                if (conn == null) {
                    stopSelf()
                    return START_NOT_STICKY
                }
                val type = if (Build.VERSION.SDK_INT >= 30) ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE else 0
                ServiceCompat.startForeground(this, 1, notification(), type)
                acquireLocks()
                startVolumeKeys()
                scope.launch {
                    // keep the notification's Mute/Unmute button in step with the real state
                    Engine.params.collect { (getSystemService(NOTIFICATION_SERVICE) as NotificationManager).notify(1, notification()) }
                }
                Engine.onTerminated = { stopSelf() }
                Engine.start(conn)
            }
        }
        return START_NOT_STICKY
    }

    private fun notification(): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, MicService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE,
        )
        val muteIntent = PendingIntent.getService(
            this, 2, Intent(this, MicService::class.java).setAction(ACTION_TOGGLE_MUTE), PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_mic)
            .setContentTitle(getString(R.string.notif_title))
            .setContentText(getString(R.string.notif_text))
            .setContentIntent(open)
            .addAction(0, getString(if (isMuted()) R.string.notif_unmute else R.string.notif_mute), muteIntent)
            .addAction(0, getString(R.string.notif_stop), stop)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .build()
    }

    private fun isMuted() = Engine.params.value?.get("mute") == true

    private fun toggleMute() {
        if (Engine.params.value == null) return
        Engine.setMuteNow(!isMuted())
        @Suppress("DEPRECATION")
        (getSystemService(VIBRATOR_SERVICE) as? Vibrator)?.vibrate(VibrationEffect.createOneShot(40, VibrationEffect.DEFAULT_AMPLITUDE))
    }

    /** Volume buttons toggle mute, also with the screen off: a media session that owns the hardware volume keys. */
    private fun startVolumeKeys() {
        if (session != null || !Store(this).volumeKeys) return
        val s = MediaSessionCompat(this, "larmamic")
        s.setPlaybackState(PlaybackStateCompat.Builder().setState(PlaybackStateCompat.STATE_PLAYING, 0, 0f).build())
        s.setPlaybackToRemote(object : VolumeProviderCompat(VOLUME_CONTROL_RELATIVE, 100, 50) {
            override fun onAdjustVolume(direction: Int) {
                if (direction != 0) toggleMute()
            }
        })
        s.isActive = true
        session = s
    }

    private fun acquireLocks() {
        if (cpu == null) {
            cpu = (getSystemService(Context.POWER_SERVICE) as PowerManager)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "larmamic:stream").apply { setReferenceCounted(false); acquire() }
        }
        if (wifi == null) {
            val mode = if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY else WifiManager.WIFI_MODE_FULL_HIGH_PERF
            wifi = (applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager)
                .createWifiLock(mode, "larmamic:wifi").apply { setReferenceCounted(false); acquire() }
        }
    }

    override fun onDestroy() {
        Engine.onTerminated = null
        Engine.stop()
        scope.cancel()
        session?.release()
        session = null
        runCatching { cpu?.release() }
        runCatching { wifi?.release() }
        cpu = null
        wifi = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val ACTION_START = "app.larmamic.START"
        const val ACTION_STOP = "app.larmamic.STOP"
        const val ACTION_TOGGLE_MUTE = "app.larmamic.TOGGLE_MUTE"
        private const val CHANNEL = "stream"
    }
}
