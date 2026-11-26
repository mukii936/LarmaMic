package app.larmamic

import android.content.Context
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import kotlinx.coroutines.flow.MutableStateFlow
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString
import org.json.JSONObject
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.X509TrustManager

/**
 * Captures the microphone and streams it to the LarmaMic PC app, and keeps the processing parameters in sync
 * both ways. Same protocol as the phone web page:
 *   phone -> PC  text {"t":"hello","rate":48000}, {"t":"set","p":{..}}, {"t":"preset"|"profile",..}, {"t":"calibrate"}, {"t":"pong"}
 *                binary: 16-bit little-endian mono PCM, 20 ms frames
 *   PC -> phone  text {"t":"ready"|"denied"|"ping"|"state"|"meter"}
 * One process-wide instance, so the foreground service and the UI share the same state.
 */
object Engine {
    val status = MutableStateFlow(Status.IDLE)
    val message = MutableStateFlow<Msg?>(null)
    val params = MutableStateFlow<Map<String, Any>?>(null)
    val preset = MutableStateFlow<String?>(null)
    val profiles = MutableStateFlow<List<String>>(emptyList())
    val profile = MutableStateFlow<String?>(null)
    val ptt = MutableStateFlow(false)
    val meter = MutableStateFlow(Meter())
    val pcLang = MutableStateFlow<String?>(null)

    /** Soundboard sounds on the PC (id, name): a button for each plays it into the microphone. */
    val sounds = MutableStateFlow<List<Pair<Int, String>>>(emptyList())

    /** "Hear myself": the PC sends the processed voice back and it plays here (use headphones). */
    val talkback = MutableStateFlow(false)
    private var track: android.media.AudioTrack? = null

    /** Called when the engine stops on its own (wrong pairing code, certificate change). */
    @Volatile
    var onTerminated: (() -> Unit)? = null

    private lateinit var app: Context
    private lateinit var store: Store
    private val main = Handler(Looper.getMainLooper())

    @Volatile private var running = false
    @Volatile private var ws: WebSocket? = null
    private var conn: Connection? = null
    private var audioThread: Thread? = null
    @Volatile private var record: AudioRecord? = null
    private var lastEdit = 0L
    private val pending = JSONObject()
    private var flushPosted = false
    private var tofu: Tofu? = null
    private var hostIdx = 0
    private var everReady = false
    val mode = MutableStateFlow("wifi")

    fun init(context: Context) {
        if (::app.isInitialized) return
        app = context.applicationContext
        store = Store(app)
        mode.value = store.mode
    }

    // ---------------------------------------------------------------- start / stop

    fun start(c: Connection) {
        if (running) return
        running = true
        conn = c
        hostIdx = 0
        everReady = false
        status.value = Status.CONNECTING
        message.value = null
        startAudioThread()
        connect()
    }

    fun stop() {
        val wasRunning = running
        running = false
        main.removeCallbacksAndMessages(null)
        flushPosted = false
        synchronized(pending) { pending.keys().asSequence().toList().forEach { pending.remove(it) } }
        ws?.let { runCatching { it.close(1000, null) } }
        ws = null
        runCatching { record?.stop() }
        audioThread?.let { t -> runCatching { t.join(500) } }
        audioThread = null
        meter.value = Meter()
        if (wasRunning || status.value != Status.ERROR) status.value = Status.IDLE
        params.value = null
        preset.value = null
        profile.value = null
        ptt.value = false
        sounds.value = emptyList()
        talkback.value = false
        runCatching { track?.stop(); track?.release() }
        track = null
    }

    private fun fail(m: Msg) {
        stop()
        status.value = Status.ERROR
        message.value = m
        onTerminated?.invoke()
    }

    // ---------------------------------------------------------------- audio

    private fun startAudioThread() {
        val t = Thread({ audioLoop() }, "larma-audio")
        t.priority = Thread.MAX_PRIORITY
        audioThread = t
        t.start()
    }

    private fun audioLoop() {
        val rate = captureRate()
        val frame = rate / 50 // 20 ms
        val am = app.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val source = if (am.getProperty(AudioManager.PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED) == "true") {
            MediaRecorder.AudioSource.UNPROCESSED
        } else {
            MediaRecorder.AudioSource.VOICE_RECOGNITION
        }
        val minBuf = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val bufBytes = maxOf(minBuf, frame * 2 * 4)
        val shorts = ShortArray(frame)
        val bytes = ByteArray(frame * 2)
        val adpcm = useAdpcm()
        val encoder = AdpcmEncoder()
        val packed = ByteArray(4 + frame / 2)

        while (running) {
            var rec: AudioRecord? = null
            try {
                rec = AudioRecord(source, rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, bufBytes)
                if (rec.state != AudioRecord.STATE_INITIALIZED) throw IllegalStateException("AudioRecord not initialised")
                rec.startRecording()
                record = rec
                // tell the PC the real rate (it resamples if it is not 48 kHz)
                ws?.send(hello(rec.sampleRate))
                while (running) {
                    val n = rec.read(shorts, 0, frame)
                    if (n <= 0) throw IllegalStateException("read failed: $n")
                    val sock = ws
                    if (adpcm && n == frame) {
                        val len = encoder.encode(shorts, n, packed)
                        if (sock != null && sock.queueSize() < 65536) sock.send(packed.toByteString(0, len))
                    } else if (!adpcm) {
                        for (i in 0 until n) {
                            val s = shorts[i].toInt()
                            bytes[2 * i] = s.toByte()
                            bytes[2 * i + 1] = (s shr 8).toByte()
                        }
                        if (sock != null && sock.queueSize() < 65536) sock.send(bytes.toByteString(0, n * 2))
                    }
                }
            } catch (_: Exception) {
                // microphone taken by a call or another app: wait and try again while we are running
                if (running) SystemClock.sleep(1000)
            } finally {
                runCatching { rec?.stop() }
                runCatching { rec?.release() }
                if (record === rec) record = null
            }
        }
    }

    // ---------------------------------------------------------------- network

    /** Trust on first use: the PC certificate is self-signed, so we pin its fingerprint the first time. */
    /**
     * Ask a PC found on the network for the pairing code. The PC shows Allow / Deny; when allowed it sends the code over
     * this TLS connection. `onDone(null)` = refused, timed out or unreachable.
     */
    fun pair(pc: Pc, onDone: (Connection?) -> Unit) {
        val key = "${pc.host}:${pc.port}"
        store.setPin(key, null) // a fresh pairing trusts the certificate it sees now
        val tm = Tofu(store, key)
        val ctx = SSLContext.getInstance("TLS").apply { init(null, arrayOf(tm), SecureRandom()) }
        val client = OkHttpClient.Builder()
            .readTimeout(75, java.util.concurrent.TimeUnit.SECONDS)
            .connectTimeout(6, java.util.concurrent.TimeUnit.SECONDS)
            .sslSocketFactory(ctx.socketFactory, tm)
            .hostnameVerifier { _, _ -> true }
            .build()
        val finished = java.util.concurrent.atomic.AtomicBoolean(false)
        val finish = { c: Connection? -> if (finished.compareAndSet(false, true)) onDone(c) }
        client.newWebSocket(
            Request.Builder().url("wss://${pc.host}:${pc.port}/pair").build(),
            object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    webSocket.send(JSONObject().put("t", "pair").put("device", android.os.Build.MODEL).toString())
                }
                override fun onMessage(webSocket: WebSocket, text: String) {
                    val m = runCatching { JSONObject(text) }.getOrNull()
                    val token = m?.optString("token").orEmpty()
                    if (m?.optString("t") == "granted" && token.isNotEmpty()) finish(Connection(pc.host, pc.port, token)) else finish(null)
                    webSocket.close(1000, null)
                }
                override fun onClosed(webSocket: WebSocket, code: Int, reason: String) { finish(null) }
                override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) { finish(null) }
            },
        )
    }

    private class Tofu(private val store: Store, private val key: String) : X509TrustManager {
        @Volatile var mismatch = false
        override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) = throw CertificateException("client")
        override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
            val fp = MessageDigest.getInstance("SHA-256").digest(chain[0].encoded).joinToString("") { "%02x".format(it) }
            val saved = store.pin(key)
            if (saved == null) {
                store.setPin(key, fp)
            } else if (saved != fp) {
                mismatch = true
                throw CertificateException("certificate changed")
            }
        }
        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    /** Forget the pinned certificate (called when the user scans a fresh QR code). */
    fun setMode(m: String) {
        store.mode = m
        mode.value = m
    }

    /** Bluetooth tethering is slow, so send 24 kHz there; the PC resamples. */
    private fun captureRate() = if (mode.value == "bt") 24000 else 48000

    /** Bluetooth tethering is slow: compress with IMA ADPCM (4:1) there. */
    private fun useAdpcm() = mode.value == "bt"

    private fun hello(rate: Int): String {
        val j = JSONObject().put("t", "hello").put("rate", rate).put("device", android.os.Build.MODEL)
        if (useAdpcm()) j.put("codec", "adpcm")
        return j.toString()
    }

    fun forgetPin(c: Connection) = store.setPin(c.key, null)

    private fun connect() {
        val c = conn ?: return
        if (!running) return
        val tm = Tofu(store, c.key)
        tofu = tm
        val ctx = SSLContext.getInstance("TLS").apply { init(null, arrayOf(tm), SecureRandom()) }
        val hosts = c.hosts(mode.value)
        val host = hosts[hostIdx % hosts.size]
        val client = OkHttpClient.Builder()
            .connectTimeout(if (hosts.size > 1) 4 else 10, java.util.concurrent.TimeUnit.SECONDS)
            .sslSocketFactory(ctx.socketFactory, tm)
            .hostnameVerifier { _, _ -> true } // identity is checked by the pinned fingerprint and the pairing code
            .retryOnConnectionFailure(true)
            .build()
        ws = client.newWebSocket(Request.Builder().url(c.wsUrl(host)).build(), listener)
    }

    private val listener = object : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            val rate = record?.sampleRate ?: 48000
            webSocket.send(hello(rate))
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            if (webSocket !== ws) return
            val m = runCatching { JSONObject(text) }.getOrNull() ?: return
            when (m.optString("t")) {
                "ready" -> {
                    if (talkback.value) webSocket.send(JSONObject().put("t", "talkback").put("on", true).toString())
                    everReady = true
                    message.value = null
                    status.value = Status.LIVE
                }
                "denied" -> main.post { fail(Msg(R.string.err_denied)) }
                "ping" -> webSocket.send(JSONObject().put("t", "pong").put("ts", m.optLong("ts")).toString())
                "state" -> onState(m)
                "meter" -> meter.value = Meter(
                    inDb = m.optDouble("in", -99.0).toFloat(),
                    outDb = m.optDouble("out", -99.0).toFloat(),
                    gate = m.optBoolean("gate", true),
                    buf = m.optInt("buf"),
                    under = m.optInt("under"),
                    drops = m.optInt("drops"),
                    cal = m.optInt("cal"),
                    calProgress = m.optDouble("calp", 0.0).toFloat(),
                    calNoise = m.optDouble("caln", -99.0).toFloat(),
                    latency = m.optInt("lat"),
                )
            }
        }

        override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
            if (webSocket !== ws || !talkback.value) return
            val t = track ?: openTrack().also { track = it }
            t.write(bytes.toByteArray(), 0, bytes.size)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) = dropped(webSocket, null)
        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) = dropped(webSocket, t)
    }

    private fun dropped(webSocket: WebSocket, error: Throwable?) {
        if (webSocket !== ws || !running) return
        if (tofu?.mismatch == true) {
            main.post { fail(Msg(R.string.err_cert)) }
            return
        }
        meter.value = Meter()
        status.value = Status.RETRY
        // never reached this address: try the PC's next one (USB / Bluetooth / hotspot address)
        val triedNext = !everReady && (conn?.hosts(mode.value)?.size ?: 1) > 1
        if (!everReady) hostIdx++
        everReady = false
        main.postDelayed({ if (running && ws === webSocket) connect() }, if (triedNext) 400 else 1500)
    }

    private fun onState(m: JSONObject) {
        m.optString("lang").takeIf { it.isNotEmpty() }?.let { pcLang.value = it }
        m.optJSONArray("profiles")?.let { arr -> profiles.value = List(arr.length()) { arr.getString(it) } }
        m.optJSONArray("sounds")?.let { arr ->
            sounds.value = List(arr.length()) { val o = arr.getJSONObject(it); o.getInt("id") to o.getString("name") }
        }
        // ignore the echo of our own edit while a slider is being dragged, so it does not jump
        if (SystemClock.elapsedRealtime() - lastEdit > 450) {
            m.optJSONObject("p")?.let { p ->
                val map = HashMap<String, Any>()
                p.keys().forEach { k ->
                    val v = p.get(k)
                    if (v is Boolean) map[k] = v else if (v is Number) map[k] = v.toDouble()
                }
                params.value = map
            }
            preset.value = if (m.isNull("preset")) null else m.optString("preset")
            profile.value = if (m.isNull("profile")) null else m.optString("profile")
        }
    }

    // ---------------------------------------------------------------- controls

    private fun send(obj: JSONObject) {
        ws?.send(obj.toString())
    }

    /** Change one parameter: shown at once, batched and sent to the PC every ~40 ms. */
    fun setParam(key: String, value: Any) {
        lastEdit = SystemClock.elapsedRealtime()
        preset.value = null
        params.value = params.value?.plus(key to value)
        synchronized(pending) { pending.put(key, value) }
        if (!flushPosted) {
            flushPosted = true
            main.postDelayed({
                flushPosted = false
                val p = synchronized(pending) {
                    val copy = JSONObject(pending.toString())
                    pending.keys().asSequence().toList().forEach { pending.remove(it) }
                    copy
                }
                send(JSONObject().put("t", "set").put("p", p))
            }, 40)
        }
    }

    fun choosePreset(name: String) {
        lastEdit = 0
        send(JSONObject().put("t", "preset").put("name", name))
    }

    fun chooseProfile(name: String) {
        lastEdit = 0
        send(JSONObject().put("t", "profile").put("name", name))
    }

    private fun openTrack(): android.media.AudioTrack {
        val min = android.media.AudioTrack.getMinBufferSize(48000, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val t = android.media.AudioTrack.Builder()
            .setAudioAttributes(
                android.media.AudioAttributes.Builder().setUsage(android.media.AudioAttributes.USAGE_MEDIA)
                    .setContentType(android.media.AudioAttributes.CONTENT_TYPE_SPEECH).build(),
            )
            .setAudioFormat(
                AudioFormat.Builder().setSampleRate(48000).setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build(),
            )
            .setBufferSizeInBytes(maxOf(min, 9600))
            .setPerformanceMode(android.media.AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            .build()
        t.play()
        return t
    }

    fun setTalkback(on: Boolean) {
        talkback.value = on
        if (!on) {
            runCatching { track?.stop(); track?.release() }
            track = null
        }
        send(JSONObject().put("t", "talkback").put("on", on))
    }

    fun playSound(id: Int) {
        send(JSONObject().put("t", "play").put("id", id))
    }

    fun stopSounds() {
        send(JSONObject().put("t", "stopsounds"))
    }

    fun calibrate() {
        lastEdit = 0
        send(JSONObject().put("t", "calibrate"))
    }

    /** Mute/unmute immediately (used by push-to-talk). */
    fun setMuteNow(mute: Boolean) {
        lastEdit = SystemClock.elapsedRealtime()
        preset.value = null
        params.value = params.value?.plus("mute" to mute)
        send(JSONObject().put("t", "set").put("p", JSONObject().put("mute", mute)))
    }

    fun enablePtt(on: Boolean) {
        ptt.value = on
        setMuteNow(on)
    }
}
