package app.larmamic

import android.content.Context
import android.content.SharedPreferences
import java.net.URI

/** The PC to connect to, taken from the address in the QR code: https://192.168.1.5:8443/?k=TOKEN */
data class Connection(val host: String, val port: Int, val token: String, val alt: List<String> = emptyList()) {
    val key get() = "$host:$port"

    /**
     * Addresses to try, in the order the PC listed them (it puts the best one for its connection mode first).
     * 127.0.0.1 only works through the USB cable (adb reverse): first in USB mode, last otherwise.
     */
    fun hosts(mode: String): List<String> {
        val all = (listOf(host) + alt).distinct()
        val (loop, rest) = all.partition { it == "127.0.0.1" }
        return if (mode == "usb") loop + rest else rest + loop
    }

    fun wsUrl(h: String) = "wss://$h:$port/ws?k=${java.net.URLEncoder.encode(token, "UTF-8")}"

    companion object {
        /** Parses a pasted or scanned address. Returns null if it has no host or pairing code. */
        fun parse(text: String): Connection? {
            val raw = text.trim()
            if (raw.isEmpty()) return null
            return try {
                val uri = URI(if (raw.contains("://")) raw else "https://$raw")
                val host = uri.host ?: return null
                val q = (uri.rawQuery ?: "").split('&').filter { it.contains('=') }
                    .associate { it.substringBefore('=') to java.net.URLDecoder.decode(it.substringAfter('='), "UTF-8") }
                val token = q["k"]
                if (token.isNullOrEmpty()) return null
                // "h" lists the PC's other addresses (USB / Bluetooth tethering, hotspot)
                val alt = (q["h"] ?: "").split(',').map { it.trim() }.filter { it.isNotEmpty() && it != host }
                Connection(host, if (uri.port > 0) uri.port else 8443, token, alt)
            } catch (_: Exception) {
                null
            }
        }
    }
}

/** Small persistent settings: the paired PC, its certificate fingerprint and the language choice. */
class Store(ctx: Context) {
    private val p: SharedPreferences = ctx.getSharedPreferences("larma", Context.MODE_PRIVATE)

    var connection: Connection?
        get() {
            val h = p.getString("host", null) ?: return null
            val t = p.getString("token", null) ?: return null
            val alt = (p.getString("alt", "") ?: "").split(',').filter { it.isNotEmpty() }
            return Connection(h, p.getInt("port", 8443), t, alt)
        }
        set(c) {
            p.edit().apply {
                if (c == null) {
                    remove("host"); remove("port"); remove("token"); remove("alt")
                } else {
                    putString("host", c.host); putInt("port", c.port); putString("token", c.token)
                    putString("alt", c.alt.joinToString(","))
                }
            }.apply()
        }

    /** "auto" follows the PC language (or the phone's). */
    var language: String
        get() = p.getString("lang", "auto") ?: "auto"
        set(v) = p.edit().putString("lang", v).apply()

    /** "wifi", "usb" or "bt": how the phone reaches the PC. Chooses the address order and the sample rate. */
    var mode: String
        get() = p.getString("mode", "wifi") ?: "wifi"
        set(v) = p.edit().putString("mode", v).apply()

    fun pin(key: String): String? = p.getString("pin:$key", null)
    fun setPin(key: String, fingerprint: String?) {
        p.edit().apply { if (fingerprint == null) remove("pin:$key") else putString("pin:$key", fingerprint) }.apply()
    }
}
