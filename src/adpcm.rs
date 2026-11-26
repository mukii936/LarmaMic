//! IMA ADPCM (4 bits per sample, 4:1 versus 16-bit PCM). Used by the Android app in Bluetooth mode to cut the
//! bandwidth. A block is: predictor (i16 LE), step index (u8), reserved (u8), then two samples per byte
//! (low nibble first). The header holds the decoder state before the first sample, so every block decodes alone.

const STEP: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157,
    173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707,
    1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635,
    13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Decodes one block into `out` (f32 in -1..1). Returns the number of samples, or 0 for a malformed block.
pub fn decode(block: &[u8], out: &mut Vec<f32>) -> usize {
    if block.len() < 4 {
        return 0;
    }
    let mut pred = i16::from_le_bytes([block[0], block[1]]) as i32;
    let mut index = (block[2] as i32).clamp(0, 88);
    let mut n = 0;
    for &byte in &block[4..] {
        for nib in [byte & 0x0f, byte >> 4] {
            let step = STEP[index as usize];
            let mut diff = step >> 3;
            if nib & 4 != 0 {
                diff += step;
            }
            if nib & 2 != 0 {
                diff += step >> 1;
            }
            if nib & 1 != 0 {
                diff += step >> 2;
            }
            pred = if nib & 8 != 0 { pred - diff } else { pred + diff }.clamp(-32768, 32767);
            index = (index + INDEX[nib as usize]).clamp(0, 88);
            out.push(pred as f32 / 32768.0);
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference encoder (the Android app has the same one in Kotlin).
    fn encode(samples: &[i16], pred: &mut i32, index: &mut i32) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + samples.len() / 2);
        out.extend_from_slice(&(*pred as i16).to_le_bytes());
        out.push(*index as u8);
        out.push(0);
        let mut low: Option<u8> = None;
        for &s in samples {
            let step = STEP[*index as usize];
            let mut diff = s as i32 - *pred;
            let mut nib = 0u8;
            if diff < 0 {
                nib = 8;
                diff = -diff;
            }
            let mut d = step >> 3;
            if diff >= step {
                nib |= 4;
                diff -= step;
                d += step;
            }
            if diff >= step >> 1 {
                nib |= 2;
                diff -= step >> 1;
                d += step >> 1;
            }
            if diff >= step >> 2 {
                nib |= 1;
                d += step >> 2;
            }
            *pred = if nib & 8 != 0 { *pred - d } else { *pred + d }.clamp(-32768, 32767);
            *index = (*index + INDEX[nib as usize]).clamp(0, 88);
            match low.take() {
                None => low = Some(nib),
                Some(l) => out.push(l | (nib << 4)),
            }
        }
        out
    }

    #[test]
    fn round_trip_of_a_voice_like_tone_stays_close() {
        let (mut pred, mut index) = (0, 0);
        let mut decoded = Vec::new();
        let mut original = Vec::new();
        for blk in 0..20 {
            let samples: Vec<i16> = (0..480)
                .map(|i| {
                    let t = (blk * 480 + i) as f32 / 24_000.0;
                    ((t * 2.0 * std::f32::consts::PI * 440.0).sin() * 9000.0 + (t * 2.0 * std::f32::consts::PI * 1300.0).sin() * 3000.0) as i16
                })
                .collect();
            let block = encode(&samples, &mut pred, &mut index);
            assert_eq!(block.len(), 4 + 240);
            assert_eq!(decode(&block, &mut decoded), 480);
            original.extend(samples.iter().map(|&s| s as f32 / 32768.0));
        }
        // skip the first block (the adaptive step needs a moment), compare the rest by error energy
        let err: f32 = original.iter().zip(&decoded).skip(480).map(|(a, b)| (a - b) * (a - b)).sum();
        let sig: f32 = original.iter().skip(480).map(|a| a * a).sum();
        assert!(err / sig < 0.01, "signal-to-error too poor: {}", err / sig);
    }

    #[test]
    fn rejects_short_blocks() {
        let mut out = Vec::new();
        assert_eq!(decode(&[0, 0], &mut out), 0);
    }
}
