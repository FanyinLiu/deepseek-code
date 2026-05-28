use std::io::Read;
use std::path::Path;

const MAX_IMAGE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

/// Encode an image file to a base64 data URL suitable for multimodal LLM APIs.
///
/// Detects MIME type from file extension (png, jpg, jpeg, gif, webp).
/// Returns a string in the format `data:image/png;base64,...`.
pub fn encode_image_to_base64(path: &Path) -> Result<String, anyhow::Error> {
    let mime = mime_type_for_path(path)?;
    let data = read_image_file_capped(path)?;

    let b64 = base64::encode(&data);
    Ok(format!("data:{mime};base64,{b64}"))
}

fn mime_type_for_path(path: &Path) -> Result<&'static str, anyhow::Error> {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        anyhow::bail!("cannot determine image type: no extension");
    };
    match ext.to_ascii_lowercase().as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        ext => anyhow::bail!("unsupported image extension: {ext}"),
    }
}

fn read_image_file_capped(path: &Path) -> Result<Vec<u8>, anyhow::Error> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_IMAGE_SIZE_BYTES {
        anyhow::bail!(
            "file too large: {} bytes, cap {}",
            metadata.len(),
            MAX_IMAGE_SIZE_BYTES
        );
    }

    let mut file = std::fs::File::open(path)?;
    let mut data = Vec::with_capacity(metadata.len() as usize);
    let bytes_read = file
        .by_ref()
        .take(MAX_IMAGE_SIZE_BYTES + 1)
        .read_to_end(&mut data)?;
    if bytes_read > MAX_IMAGE_SIZE_BYTES as usize {
        anyhow::bail!("file too large: {bytes_read} bytes, cap {MAX_IMAGE_SIZE_BYTES}");
    }
    Ok(data)
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
    fn test_uppercase_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.PNG");
        std::fs::write(&path, base64::decode(BASE64_1X1_PNG)).unwrap();

        let result = encode_image_to_base64(&path).unwrap();
        assert!(result.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn test_large_image_rejected_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.png");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_IMAGE_SIZE_BYTES + 1).unwrap();

        let error = encode_image_to_base64(&path).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "file too large: {} bytes, cap {}",
                MAX_IMAGE_SIZE_BYTES + 1,
                MAX_IMAGE_SIZE_BYTES
            )
        );
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
