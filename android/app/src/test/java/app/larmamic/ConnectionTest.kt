package app.larmamic

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ConnectionTest {
    @Test
    fun parsesAddressFromQr() {
        val c = Connection.parse("https://192.168.1.5:8443/?k=abc123")!!
        assertEquals("192.168.1.5", c.host)
        assertEquals(8443, c.port)
        assertEquals("abc123", c.token)
        assertEquals(emptyList<String>(), c.alt)
    }

    @Test
    fun readsExtraAddressesForUsbAndBluetooth() {
        val c = Connection.parse("https://192.168.1.5:8443/?k=tok&h=192.168.42.1,10.0.0.2")!!
        assertEquals(listOf("192.168.42.1", "10.0.0.2"), c.alt)
        assertEquals(listOf("192.168.1.5", "192.168.42.1", "10.0.0.2"), c.hosts("wifi"))
        assertEquals(listOf("192.168.1.5", "192.168.42.1", "10.0.0.2"), c.hosts("bt"))
    }

    @Test
    fun acceptsPastedAddressWithoutScheme() {
        val c = Connection.parse("  10.0.0.9:9443/?k=zz  ")!!
        assertEquals("10.0.0.9", c.host)
        assertEquals(9443, c.port)
    }

    @Test
    fun rejectsAddressWithoutPairingCode() {
        assertNull(Connection.parse("https://192.168.1.5:8443/"))
        assertNull(Connection.parse(""))
    }

    @Test
    fun buildsWebSocketUrlForAnyHost() {
        val c = Connection("10.0.0.2", 8443, "a b")
        assertEquals("wss://192.168.42.1:8443/ws?k=a+b", c.wsUrl("192.168.42.1"))
    }

    @Test
    fun usbCableLinkThroughAdbUsesLoopbackFirst() {
        val c = Connection("127.0.0.1", 8443, "t", listOf("192.168.42.1", "192.168.1.5"))
        assertEquals(listOf("127.0.0.1", "192.168.42.1", "192.168.1.5"), c.hosts("usb"))
        assertEquals(listOf("192.168.42.1", "192.168.1.5", "127.0.0.1"), c.hosts("wifi"))
    }
}
