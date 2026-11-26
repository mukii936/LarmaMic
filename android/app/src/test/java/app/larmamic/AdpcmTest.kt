package app.larmamic

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.PI
import kotlin.math.sin

class AdpcmTest {
    private fun decode(block: ByteArray, len: Int): ShortArray {
        var pred = (block[0].toInt() and 0xff) or (block[1].toInt() shl 8)
        var index = block[2].toInt() and 0xff
        val out = ShortArray((len - 4) * 2)
        var n = 0
        for (b in 4 until len) {
            val byte = block[b].toInt() and 0xff
            for (nib in intArrayOf(byte and 0x0f, byte shr 4)) {
                val step = AdpcmEncoder.STEP[index]
                var diff = step shr 3
                if (nib and 4 != 0) diff += step
                if (nib and 2 != 0) diff += step shr 1
                if (nib and 1 != 0) diff += step shr 2
                pred = (if (nib and 8 != 0) pred - diff else pred + diff).coerceIn(-32768, 32767)
                index = (index + AdpcmEncoder.INDEX[nib]).coerceIn(0, 88)
                out[n++] = pred.toShort()
            }
        }
        return out
    }

    @Test
    fun roundTripStaysClose() {
        val enc = AdpcmEncoder()
        var err = 0.0
        var sig = 0.0
        for (blk in 0 until 20) {
            val s = ShortArray(480) { i ->
                val t = (blk * 480 + i) / 24000.0
                (sin(t * 2 * PI * 440) * 9000 + sin(t * 2 * PI * 1300) * 3000).toInt().toShort()
            }
            val out = ByteArray(4 + 240)
            assertEquals(244, enc.encode(s, 480, out))
            val dec = decode(out, 244)
            if (blk > 0) for (i in 0 until 480) {
                val d = (s[i] - dec[i]).toDouble()
                err += d * d
                sig += s[i].toDouble() * s[i]
            }
        }
        assertTrue("error ratio ${err / sig}", err / sig < 0.01)
    }
}
