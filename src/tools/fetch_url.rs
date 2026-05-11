//! Fetch URL content and convert to plain text for the model.
use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::{header::LOCATION, redirect::Policy, Url};

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// Fetch a URL and return its content as plain text.
/// HTML is stripped to basic text. Max output is limited to avoid token overflow.
pub async fn fetch_url(url: &str, max_chars: usize) -> Result<String, anyhow::Error> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(Policy::none())
        .user_agent("deepseek-code/0.1.0")
        .build()?;

    let mut current_url = parse_and_validate_url(url).await?;

    for _ in 0..=MAX_REDIRECTS {
        let response = client.get(current_url.clone()).send().await?;
        let status = response.status();

        if status.is_redirection() {
            let next_url = redirect_target(&current_url, &response)?;
            validate_url_allowed(&next_url).await?;
            current_url = next_url;
            continue;
        }

        if !status.is_success() {
            anyhow::bail!("HTTP {status} for {current_url}");
        }

        if let Some(content_length) = response.content_length() {
            if content_length > MAX_RESPONSE_BYTES as u64 {
                anyhow::bail!(
                    "response too large: {content_length} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"
                );
            }
        }

        let body_bytes = collect_limited_body(response.bytes_stream(), MAX_RESPONSE_BYTES).await?;
        let body = String::from_utf8_lossy(&body_bytes);
        let text = html_to_text(&body);

        let trimmed = if text.chars().count() > max_chars {
            let mut truncated: String = text.chars().take(max_chars).collect();
            truncated.push_str("\n\n[Content truncated — fetch full page for more]");
            truncated
        } else {
            text
        };

        return Ok(format!("Fetched: {current_url}\n\n{trimmed}"));
    }

    anyhow::bail!("too many redirects fetching {url}");
}

async fn parse_and_validate_url(url: &str) -> Result<Url, anyhow::Error> {
    let parsed = Url::parse(url)?;
    validate_url_allowed(&parsed).await?;
    Ok(parsed)
}

fn redirect_target(base: &Url, response: &reqwest::Response) -> Result<Url, anyhow::Error> {
    let location = response
        .headers()
        .get(LOCATION)
        .ok_or_else(|| anyhow::anyhow!("redirect missing Location header"))?
        .to_str()?;
    Ok(base.join(location)?)
}

async fn validate_url_allowed(url: &Url) -> Result<(), anyhow::Error> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => anyhow::bail!("unsupported URL scheme: {scheme}"),
    }

    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL is missing a host"))?;
    let normalized_host = host.trim_end_matches('.').to_ascii_lowercase();

    if normalized_host == "localhost" || normalized_host.ends_with(".localhost") {
        anyhow::bail!("blocked local/private/link-local address: {host}");
    }

    if let Ok(ip) = normalized_host.parse::<std::net::IpAddr>() {
        if is_blocked_ip(ip) {
            anyhow::bail!("blocked local/private/link-local address: {ip}");
        }
        return Ok(());
    }

    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("URL is missing a port for scheme {}", url.scheme()))?;
    let addrs = tokio::net::lookup_host((host, port)).await?;
    for addr in addrs {
        if is_blocked_ip(addr.ip()) {
            anyhow::bail!("blocked local/private/link-local address: {}", addr.ip());
        }
    }

    Ok(())
}

fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        std::net::IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_blocked_ip(std::net::IpAddr::V4(mapped));
            }

            let first_segment = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || (first_segment & 0xfe00) == 0xfc00
                || (first_segment & 0xffc0) == 0xfe80
        }
    }
}

async fn collect_limited_body<S, E>(stream: S, max_bytes: usize) -> Result<Vec<u8>, anyhow::Error>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    futures::pin_mut!(stream);

    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            anyhow::bail!("response too large: exceeds {max_bytes} byte limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Very basic HTML-to-text conversion.
/// Removes tags, decodes common entities, preserves line breaks.
/// Skips content inside <script> and <style> tags.
fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut in_skip = false;
    let mut tag_buffer = String::new();
    let mut prev_char = ' ';

    for c in html.chars() {
        if in_skip {
            if c == '<' {
                tag_buffer.clear();
                tag_buffer.push(c);
                in_tag = true;
            } else if in_tag {
                tag_buffer.push(c);
                if c == '>' {
                    let lower = tag_buffer.to_lowercase();
                    if lower.starts_with("</script>") || lower.starts_with("</style>") {
                        in_skip = false;
                    }
                    in_tag = false;
                    tag_buffer.clear();
                }
            }
            continue;
        }

        if c == '<' {
            in_tag = true;
            tag_buffer.clear();
            tag_buffer.push(c);
            continue;
        }

        if c == '>' && in_tag {
            in_tag = false;
            let lower = tag_buffer.to_lowercase();
            if lower.starts_with("<script") || lower.starts_with("<style") {
                in_skip = true;
            }
            tag_buffer.clear();
            continue;
        }

        if in_tag {
            tag_buffer.push(c);
            continue;
        }

        // Normalize whitespace
        if c.is_whitespace() {
            if prev_char != ' ' {
                text.push(' ');
                prev_char = ' ';
            }
        } else {
            text.push(c);
            prev_char = c;
        }
    }

    // Decode common entities
    let text = text.replace("&lt;", "<");
    let text = text.replace("&gt;", ">");
    let text = text.replace("&amp;", "&");
    let text = text.replace("&quot;", "\"");
    let text = text.replace("&#39;", "'");
    let text = text.replace("&nbsp;", " ");

    // Collapse multiple blank lines
    let lines: Vec<&str> = text.lines().collect();
    let mut result = String::new();
    let mut blank_count = 0;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            result.push_str(trimmed);
            result.push('\n');
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_text_basic() {
        let html = "<html><body><p>Hello <b>world</b>!</p></body></html>";
        let text = html_to_text(html);
        assert_eq!(text, "Hello world!");
    }

    #[test]
    fn test_html_to_text_skips_script() {
        let html = "<p>Before</p><script>alert('x');</script><p>After</p>";
        let text = html_to_text(html);
        assert!(text.contains("Before"), "Expected 'Before' in: {text}");
        assert!(text.contains("After"), "Expected 'After' in: {text}");
        assert!(!text.contains("alert"), "Did not expect 'alert' in: {text}");
    }

    #[tokio::test]
    async fn test_validate_url_blocks_localhost() {
        let url = Url::parse("http://localhost/").expect("url");
        let err = validate_url_allowed(&url)
            .await
            .expect_err("localhost should be blocked");
        assert!(
            err.to_string().contains("blocked local/private/link-local"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_blocked_private_and_link_local_ips() {
        assert!(is_blocked_ip("127.0.0.1".parse().expect("ip")));
        assert!(is_blocked_ip("10.0.0.1".parse().expect("ip")));
        assert!(is_blocked_ip("169.254.169.254".parse().expect("ip")));
        assert!(is_blocked_ip("fe80::1".parse().expect("ip")));
        assert!(is_blocked_ip("fc00::1".parse().expect("ip")));
    }

    #[tokio::test]
    async fn test_collect_limited_body_rejects_large_response() {
        let chunks = futures::stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(b"12345")),
            Ok::<_, std::io::Error>(Bytes::from_static(b"67890")),
        ]);

        let err = collect_limited_body(chunks, 8)
            .await
            .expect_err("body over limit should fail");
        assert!(
            err.to_string().contains("response too large"),
            "unexpected error: {err}"
        );
    }
}
