package app.larmamic

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.os.Handler
import android.os.Looper
import kotlinx.coroutines.flow.MutableStateFlow
import java.net.Inet4Address

/** A LarmaMic PC found on the network (mDNS service `_larmamic._tcp`). */
data class Pc(val name: String, val host: String, val port: Int) {
    val label get() = name.removePrefix("LarmaMic ").ifBlank { host }
}

/** Looks for PCs on the local network so the phone can ask to pair without scanning a QR code. */
object Discovery {
    val found = MutableStateFlow<List<Pc>>(emptyList())
    private var nsd: NsdManager? = null
    private var listener: NsdManager.DiscoveryListener? = null
    private val main = Handler(Looper.getMainLooper())
    private val queue = ArrayDeque<NsdServiceInfo>()
    private var resolving = false

    fun start(ctx: Context) {
        if (listener != null) return
        val m = ctx.applicationContext.getSystemService(Context.NSD_SERVICE) as NsdManager
        nsd = m
        val l = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {}
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                main.post { queue.add(serviceInfo); next() }
            }
            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                found.value = found.value.filter { it.name != serviceInfo.serviceName }
            }
        }
        listener = l
        runCatching { m.discoverServices("_larmamic._tcp.", NsdManager.PROTOCOL_DNS_SD, l) }
    }

    /** Android resolves one service at a time. */
    private fun next() {
        if (resolving) return
        val info = queue.removeFirstOrNull() ?: return
        val m = nsd ?: return
        resolving = true
        val done = { resolving = false; next() }
        val rl = object : NsdManager.ResolveListener {
            override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) { main.post(done) }
            override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                val host = if (Build.VERSION.SDK_INT >= 34) {
                    serviceInfo.hostAddresses.firstOrNull { it is Inet4Address }
                } else {
                    @Suppress("DEPRECATION") serviceInfo.host?.takeIf { it is Inet4Address }
                }?.hostAddress
                if (host != null) {
                    val pc = Pc(serviceInfo.serviceName, host, serviceInfo.port)
                    found.value = found.value.filter { it.name != pc.name } + pc
                }
                main.post(done)
            }
        }
        runCatching { @Suppress("DEPRECATION") m.resolveService(info, rl) }.onFailure { resolving = false }
    }

    fun stop() {
        listener?.let { l -> runCatching { nsd?.stopServiceDiscovery(l) } }
        listener = null
        queue.clear()
        resolving = false
        found.value = emptyList()
    }
}
