/// Encode a string as URL-safe base64 (no line wrapping) for safe shell transfer.
pub fn base64_encode(s: &str) -> String {
    use std::fmt::Write as _;
    let bytes = s.as_bytes();
    let mut out = String::new();
    // simple base64 alphabet
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        let _ = write!(out, "{}", ALPHA[((n >> 18) & 0x3F) as usize] as char);
        let _ = write!(out, "{}", ALPHA[((n >> 12) & 0x3F) as usize] as char);
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 1 {
                ALPHA[((n >> 6) & 0x3F) as usize] as char
            } else {
                '='
            }
        );
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 2 {
                ALPHA[(n & 0x3F) as usize] as char
            } else {
                '='
            }
        );
    }
    out
}
