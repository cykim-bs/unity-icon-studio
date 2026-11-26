//! Small image helpers for captures: thumbnails and `data:` URLs for the window.

use std::path::Path;

/// A copy of the PNG at `src`, at most `size` pixels on its long side, written to `dst` as PNG.
pub fn thumbnail(src: &Path, dst: &Path, size: u32) -> Result<(), String> {
    let img = ::image::open(src).map_err(|e| format!("image:{e}"))?;
    let thumb = img.thumbnail(size, size);
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("image:{e}"))?;
    }
    thumb.save_with_format(dst, ::image::ImageFormat::Png).map_err(|e| format!("image:{e}"))
}

/// `data:image/png;base64,...` of a PNG file.
pub fn png_data_url(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read:{e}"))?;
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("notPng".into());
    }
    Ok(format!("data:image/png;base64,{}", base64(&bytes)))
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_known_values() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
