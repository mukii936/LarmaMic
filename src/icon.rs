//! Icon vẽ bằng code (micro trắng trên nền xanh ngọc), dùng cho khay hệ thống và cửa sổ.

fn seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (vx, vy, wx, wy) = (bx - ax, by - ay, px - ax, py - ay);
    let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy).max(1e-9)).clamp(0.0, 1.0);
    ((px - (ax + t * vx)).powi(2) + (py - (ay + t * vy)).powi(2)).sqrt()
}

pub fn rgba(n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n * n * 4];
    let aa = 1.5 / n as f32;
    let cov = |d: f32| (0.5 - d / aa).clamp(0.0, 1.0);
    let (teal, dark) = ([0x38u8, 0xe0, 0xc8], [0x04u8, 0x20, 0x1c]);
    for py in 0..n {
        for px in 0..n {
            let (x, y) = ((px as f32 + 0.5) / n as f32, (py as f32 + 0.5) / n as f32);
            let a_disk = cov(((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() - 0.48);
            let r = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
            let d_capsule = seg(x, y, 0.5, 0.30, 0.5, 0.46) - 0.11;
            let d_arc = if y >= 0.50 { (r - 0.20).abs() - 0.028 } else { f32::MAX };
            let d_stem = seg(x, y, 0.5, 0.70, 0.5, 0.78) - 0.028;
            let d_base = seg(x, y, 0.40, 0.79, 0.60, 0.79) - 0.028;
            let a_glyph = cov(d_capsule.min(d_arc).min(d_stem).min(d_base));
            let i = (py * n + px) * 4;
            for c in 0..3 {
                out[i + c] = (teal[c] as f32 * (1.0 - a_glyph) + dark[c] as f32 * a_glyph) as u8;
            }
            out[i + 3] = (a_disk * 255.0) as u8;
        }
    }
    out
}
