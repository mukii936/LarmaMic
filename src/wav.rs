//! Ghi file WAV 16 bit mono 48 kHz. Tiêu đề được vá lại kích thước khi kết thúc.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

pub struct WavWriter {
    f: BufWriter<File>,
    data_bytes: u32,
}

impl WavWriter {
    pub fn create(path: &Path, rate: u32) -> std::io::Result<Self> {
        let mut f = BufWriter::new(File::create(path)?);
        f.write_all(b"RIFF")?;
        f.write_all(&0u32.to_le_bytes())?; // kích thước: vá sau
        f.write_all(b"WAVEfmt ")?;
        f.write_all(&16u32.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?; // PCM
        f.write_all(&1u16.to_le_bytes())?; // mono
        f.write_all(&rate.to_le_bytes())?;
        f.write_all(&(rate * 2).to_le_bytes())?; // byte/giây
        f.write_all(&2u16.to_le_bytes())?; // block align
        f.write_all(&16u16.to_le_bytes())?; // bit/mẫu
        f.write_all(b"data")?;
        f.write_all(&0u32.to_le_bytes())?; // kích thước dữ liệu: vá sau
        Ok(Self { f, data_bytes: 0 })
    }

    pub fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            self.f.write_all(&v.to_le_bytes())?;
        }
        self.data_bytes = self.data_bytes.saturating_add(samples.len() as u32 * 2);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn seconds(&self, rate: u32) -> f32 {
        self.data_bytes as f32 / 2.0 / rate as f32
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        self.f.flush()?;
        self.f.seek(SeekFrom::Start(4))?;
        self.f.write_all(&(36 + self.data_bytes).to_le_bytes())?;
        self.f.seek(SeekFrom::Start(40))?;
        self.f.write_all(&self.data_bytes.to_le_bytes())?;
        self.f.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_size_are_patched() {
        let path = std::env::temp_dir().join("larma-wav-test.wav");
        let mut w = WavWriter::create(&path, 48_000).unwrap();
        w.write(&vec![0.5f32; 4800]).unwrap();
        assert!((w.seconds(48_000) - 0.1).abs() < 1e-6);
        w.finish().unwrap();
        let b = std::fs::read(&path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()), 9600);
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 36 + 9600);
        assert_eq!(b.len(), 44 + 9600);
        // mẫu đầu ≈ 0,5 · 32767
        let s0 = i16::from_le_bytes([b[44], b[45]]);
        assert!((s0 as i32 - 16383).abs() < 3, "mẫu đầu {s0}");
        let _ = std::fs::remove_file(&path);
    }
}
