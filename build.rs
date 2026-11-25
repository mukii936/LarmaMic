//! Lúc build: sinh file .ico nhiều cỡ từ cùng hàm vẽ icon dùng cho cửa sổ/khay (src/icon.rs),
//! rồi nhúng vào larma-mic.exe để Explorer, thanh tác vụ và Alt+Tab đều hiện đúng icon.

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

use std::path::Path;

/// ICO chứa ảnh BMP 32 bit (BGRA, xếp từ dưới lên) cho từng cỡ.
fn make_ico(sizes: &[usize]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&n| {
            let rgba = icon::rgba(n);
            let mask_row = n.div_ceil(32) * 4;
            let mut img = Vec::with_capacity(40 + n * n * 4 + mask_row * n);
            // BITMAPINFOHEADER; chiều cao tính gấp đôi vì có thêm mặt nạ AND
            img.extend_from_slice(&40u32.to_le_bytes());
            img.extend_from_slice(&(n as i32).to_le_bytes());
            img.extend_from_slice(&((n * 2) as i32).to_le_bytes());
            img.extend_from_slice(&1u16.to_le_bytes());
            img.extend_from_slice(&32u16.to_le_bytes());
            img.extend_from_slice(&0u32.to_le_bytes());
            img.extend_from_slice(&((n * n * 4 + mask_row * n) as u32).to_le_bytes());
            img.extend_from_slice(&[0u8; 16]);
            for y in (0..n).rev() {
                for x in 0..n {
                    let i = (y * n + x) * 4;
                    img.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            img.extend(std::iter::repeat(0u8).take(mask_row * n)); // độ trong suốt đã nằm ở kênh alpha
            img
        })
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (&n, img) in sizes.iter().zip(&images) {
        out.push(if n >= 256 { 0 } else { n as u8 });
        out.push(if n >= 256 { 0 } else { n as u8 });
        out.push(0); // số màu
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bit count
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in &images {
        out.extend_from_slice(img);
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = std::env::var("OUT_DIR").expect("thiếu OUT_DIR");
    let ico = Path::new(&out_dir).join("larma.ico");
    std::fs::write(&ico, make_ico(&[16, 24, 32, 48, 64, 128, 256])).expect("không ghi được larma.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().expect("đường dẫn icon không hợp lệ"));
        res.set("ProductName", "LarmaMic");
        res.set("FileDescription", "LarmaMic: use your iPhone as a PC microphone");
        res.set("InternalName", "larma-mic");
        res.set("OriginalFilename", "larma-mic.exe");
        if let Err(e) = res.compile() {
            // Không có công cụ nhúng tài nguyên (rc.exe): vẫn build được, chỉ là file .exe không có icon
            println!("cargo:warning=không nhúng được icon vào exe: {e}");
        }
    }
}
