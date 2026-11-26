package app.larmamic

/**
 * IMA ADPCM encoder (4 bits per sample). Mirror of src/adpcm.rs on the PC. A block is: predictor (i16 LE), step index (u8),
 * reserved (u8), then two samples per byte (low nibble first). The header holds the state before the first sample.
 */
class AdpcmEncoder {
    private var pred = 0
    private var index = 0

    /** Encodes `n` samples (must be even) into `out`, returns the number of bytes written (4 + n / 2). */
    fun encode(src: ShortArray, n: Int, out: ByteArray): Int {
        out[0] = pred.toByte()
        out[1] = (pred shr 8).toByte()
        out[2] = index.toByte()
        out[3] = 0
        var o = 4
        var low = -1
        for (i in 0 until n) {
            val step = STEP[index]
            var diff = src[i].toInt() - pred
            var nib = 0
            if (diff < 0) {
                nib = 8
                diff = -diff
            }
            var d = step shr 3
            if (diff >= step) { nib = nib or 4; diff -= step; d += step }
            if (diff >= step shr 1) { nib = nib or 2; diff -= step shr 1; d += step shr 1 }
            if (diff >= step shr 2) { nib = nib or 1; d += step shr 2 }
            pred = (if (nib and 8 != 0) pred - d else pred + d).coerceIn(-32768, 32767)
            index = (index + INDEX[nib]).coerceIn(0, 88)
            if (low < 0) low = nib else { out[o++] = (low or (nib shl 4)).toByte(); low = -1 }
        }
        return o
    }

    companion object {
        val STEP = intArrayOf(
            7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157,
            173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707,
            1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635,
            13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
        )
        val INDEX = intArrayOf(-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8)
    }
}
