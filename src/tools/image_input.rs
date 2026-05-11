use std::path::Path;

/// Encode an image file to a base64 data URL suitable for multimodal LLM APIs.
///
/// Detects MIME type from file extension (png, jpg, jpeg, gif, webp).
/// Returns a string in the format `data:image/png;base64,...`.
pub fn encode_image_to_base64(path: &Path) -> Result<String, anyhow::Error> {
    let data = std::fs::read(path)?;

    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some(ext) => anyhow::bail!("unsupported image extension: {ext}"),
        None => anyhow::bail!("cannot determine image type: no extension"),
    };

    let b64 = base64::encode(&data);
    Ok(format!("data:{mime};base64,{b64}"))
}

// Inline base64 encoder to avoid adding a new dependency.
// If the project already has a base64 crate, this module can be removed.
mod base64 {
    pub fn encode(input: &[u8]) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b = match chunk.len() {
                1 => [chunk[0], 0, 0],
                2 => [chunk[0], chunk[1], 0],
                3 => [chunk[0], chunk[1], chunk[2]],
                _ => unreachable!(),
            };
            out.push(CHARS[(b[0] >> 2) as usize] as char);
            out.push(CHARS[(((b[0] & 0x3) << 4) | (b[1] >> 4)) as usize] as char);
            out.push(if chunk.len() > 1 {
                CHARS[(((b[1] & 0xF) << 2) | (b[2] >> 6)) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                CHARS[(b[2] & 0x3F) as usize] as char
            } else {
                '='
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A minimal 1x1 PNG file, base64-encoded.
    const BASE64_1X1_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVQI12P4z8AAAAADAAEABf7YIgAAAABJRU5ErkJggg==";

    #[test]
    fn test_encode_image_to_base64_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&base64::decode(BASE64_1X1_PNG)).unwrap();
        }

        let result = encode_image_to_base64(&path).unwrap();
        assert!(result.starts_with("data:image/png;base64,"));
        let encoded_part = result.strip_prefix("data:image/png;base64,").unwrap();
        // The helper should produce the same base64 string we started with.
        assert_eq!(encoded_part, BASE64_1X1_PNG);
    }

    #[test]
    fn test_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bmp");
        std::fs::write(&path, b"dummy").unwrap();
        assert!(encode_image_to_base64(&path).is_err());
    }

    #[test]
    fn test_no_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test");
        std::fs::write(&path, b"dummy").unwrap();
        assert!(encode_image_to_base64(&path).is_err());
    }

    mod base64 {
        pub fn decode(input: &str) -> Vec<u8> {
            // Simple base64 decoder for test data
            let mut out = Vec::with_capacity(input.len() * 3 / 4);
            let mut buf = 0u32;
            let mut bits = 0u32;
            for c in input.chars() {
                let val = match c {
                    'A'..='Z' => c as u8 - b'A',
                    'a'..='z' => c as u8 - b'a' + 26,
                    '0'..='9' => c as u8 - b'0' + 52,
                    '+' => 62,
                    '/' => 63,
                    '=' => continue,
                    _ => continue,
                };
                buf = (buf << 6) | val as u32;
                bits += 6;
                if bits >= 8 {
                    bits -= 8;
                    out.push((buf >> bits) as u8);
                }
            }
            out
        }
    }
}
