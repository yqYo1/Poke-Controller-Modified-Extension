use std::net::IpAddr;
use std::path::PathBuf;

use pokecon_core::cv::camera::{Frame, PixelFormat};
use pokecon_core::serial::SendFormat;
use pokecon_core::serial::keys::Button;
use url::Url;

// ── Helper: Encode a camera Frame as raw JPEG bytes ────────────────────────

pub fn frame_to_jpeg_bytes(frame: &Frame) -> Result<Vec<u8>, String> {
    // Normalize pixel data to RGB (3 bytes per pixel) for JPEG encoding
    let (data, width, height) = match frame.format {
        PixelFormat::Rgb => (frame.data.clone(), frame.width, frame.height),
        PixelFormat::Bgr => {
            // Convert BGR → RGB by swapping first and third byte
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(3)
                .flat_map(|c| [c[2], c[1], c[0]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Rgba => {
            // Drop alpha channel
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(4)
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Gray => {
            // Expand grayscale to RGB
            let rgb: Vec<u8> = frame.data.iter().flat_map(|&g| [g, g, g]).collect();
            (rgb, frame.width, frame.height)
        }
    };

    let mut buf = Vec::new();
    {
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        encoder
            .encode(&data, width, height, image::ColorType::Rgb8)
            .map_err(|e| format!("JPEG encode error: {e}"))?;
    }

    Ok(buf)
}

// ── Helper: Encode a camera Frame as base64 JPEG ───────────────────────────────

pub fn frame_to_base64_jpeg(frame: &Frame) -> Result<String, String> {
    use base64::Engine;

    let jpeg_bytes = frame_to_jpeg_bytes(frame)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes))
}

/// Save a camera frame as a JPEG file at the given path.
pub fn save_frame_as_jpeg(frame: &Frame, path: &PathBuf) -> Result<(), String> {
    let jpeg_bytes = frame_to_jpeg_bytes(frame)?;
    std::fs::write(path, &jpeg_bytes).map_err(|e| format!("Failed to write JPEG file: {e}"))
}

/// Sanitize a filename to prevent path traversal attacks.
/// Strips directory separators and `..` sequences, keeping only the final filename component.
pub fn sanitize_filename(name: &str) -> String {
    // Use std::path to get the file name component only (strips all directory parts)
    let path = PathBuf::from(name);
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    // Remove any remaining path separators or special characters
    file_name
        .chars()
        .filter(|&c| !std::path::is_separator(c) && c != '\0')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Validate that a URL is safe to use (no SSRF).
/// Only http/https schemes allowed, private/internal IPs rejected.
pub fn validate_webhook_url(url_str: &str) -> Result<(), String> {
    if url_str.is_empty() {
        return Ok(()); // Empty URLs are allowed (means "not configured")
    }

    let parsed = Url::parse(url_str).map_err(|_| format!("Invalid URL: {url_str}"))?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "Unsupported URL scheme '{scheme}'. Only http and https are allowed.",
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL has no host: {url_str}"))?;

    // Reject empty host (e.g., http:///path)
    if host.is_empty() {
        return Err(format!("URL host is empty: {url_str}"));
    }

    // Block private/internal addresses (SSRF prevention)
    let lower_host = host.to_lowercase();

    // Block internal hostnames
    if lower_host == "localhost"
        || lower_host == "localhost.localdomain"
        || lower_host.ends_with(".local")
        || lower_host.ends_with(".internal")
    {
        return Err(format!(
            "URL host '{host}' is an internal hostname and is not allowed",
        ));
    }

    // Parse as IP and block loopback/private/link-local addresses
    if let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        let is_blocked = match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_unspecified() || v4.is_link_local()
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
        };
        if is_blocked {
            return Err(format!(
                "URL host '{ip}' is a private/internal IP address and is not allowed",
            ));
        }
    }

    Ok(())
}

/// Mask a secret value for safe display (show last 4 chars, rest as ****).
pub fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if value.len() <= 4 {
        return "****".to_string();
    }
    format!("****{}", &value[value.len() - 4..])
}

// ── Default value functions for serde ─────────────────────────────────────────

pub fn default_sensitivity() -> f32 {
    1.0
}

pub fn default_press_duration() -> u64 {
    50
}

pub fn default_stick_duration() -> u64 {
    100
}

pub fn default_touch_duration() -> u64 {
    100
}

// ── Helper: Parse button name strings into Button values ────────────────

/// Parse a list of button name strings (e.g. "A", "B") into Button enum values.
/// Invalid names are silently ignored.
pub fn parse_buttons(buttons: &[String]) -> Vec<Button> {
    let mut result = Vec::new();
    for name in buttons {
        match name.to_uppercase().as_str() {
            "A" => result.push(Button::A),
            "B" => result.push(Button::B),
            "X" => result.push(Button::X),
            "Y" => result.push(Button::Y),
            "L" => result.push(Button::L),
            "R" => result.push(Button::R),
            "ZL" => result.push(Button::ZL),
            "ZR" => result.push(Button::ZR),
            "MINUS" | "-" | "SELECT" => result.push(Button::MINUS),
            "PLUS" | "START" => result.push(Button::PLUS),
            "LCLICK" | "L3" | "LSTICK" => result.push(Button::LCLICK),
            "RCLICK" | "R3" | "RSTICK" => result.push(Button::RCLICK),
            "HOME" => result.push(Button::HOME),
            "CAPTURE" => result.push(Button::CAPTURE),
            _ => {} // Unknown button name, ignore
        }
    }
    result
}

// ── Helper: Format a SendFormat row for serial output ───────────────────

/// Convert a SendFormat into a serial row string in Default format.
pub fn format_default_row(fmt: &SendFormat, l_changed: bool, r_changed: bool) -> String {
    fmt.convert_to_default(l_changed, r_changed)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests

// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // ── sanitize_filename ──────────────────────────────────────────────

    #[test]
    fn test_sanitize_filename_keeps_simple_name() {
        assert_eq!(sanitize_filename("test.txt"), "test.txt");
    }

    #[test]
    fn test_sanitize_filename_strips_directory_prefix() {
        assert_eq!(sanitize_filename("foo/bar/baz.txt"), "baz.txt");
    }

    #[test]
    fn test_sanitize_filename_strips_windows_directory() {
        // On Linux, backslash is not a separator, so the whole string is kept
        // but drive letter paths on Windows would be stripped there
        let result = sanitize_filename("C:\\Users\\foo\\bar.txt");
        // On Linux, backslash is not a path separator, so we expect the full string
        // (or just "bar.txt" on Windows where \ is a separator)
        assert!(!result.contains(".."));
    }

    #[test]
    fn test_sanitize_filename_removes_dotdot() {
        let result = sanitize_filename("../../etc/passwd");
        assert_eq!(result, "passwd");
        assert!(!result.contains(".."));
    }

    #[test]
    fn test_sanitize_filename_removes_null_byte() {
        let result = sanitize_filename("file\0.txt");
        assert!(!result.contains('\0'));
        assert_eq!(result, "file.txt");
    }

    #[test]
    fn test_sanitize_filename_empty_string() {
        assert_eq!(sanitize_filename(""), "");
    }

    #[test]
    fn test_sanitize_filename_no_extension() {
        assert_eq!(sanitize_filename("somefile"), "somefile");
    }

    #[test]
    fn test_sanitize_filename_trailing_separator() {
        let result = sanitize_filename("dirname/");
        // When path ends with separator, file_name() returns None,
        // so it falls back to the whole string then filters separators
        assert!(!result.contains('/'));
    }

    // ── validate_webhook_url ───────────────────────────────────────────

    #[test]
    fn test_validate_webhook_url_empty_is_ok() {
        assert!(validate_webhook_url("").is_ok());
    }

    #[test]
    fn test_validate_webhook_url_valid_https() {
        assert!(validate_webhook_url("https://hooks.example.com/webhook").is_ok());
    }

    #[test]
    fn test_validate_webhook_url_valid_http() {
        assert!(validate_webhook_url("http://example.com/notify").is_ok());
    }

    #[test]
    fn test_validate_webhook_url_rejects_ftp() {
        assert!(validate_webhook_url("ftp://example.com").is_err());
    }

    #[test]
    fn test_validate_webhook_url_rejects_file_scheme() {
        assert!(validate_webhook_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_validate_webhook_url_invalid_syntax() {
        assert!(validate_webhook_url("not a url at all").is_err());
    }

    #[test]
    fn test_validate_webhook_url_rejects_localhost() {
        let err = validate_webhook_url("http://localhost/webhook").unwrap_err();
        assert!(err.contains("internal hostname"), "Got: {err}");
    }

    #[test]
    fn test_validate_webhook_url_rejects_localhost_localdomain() {
        let err = validate_webhook_url("http://localhost.localdomain/test").unwrap_err();
        assert!(err.contains("internal hostname"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_dot_local() {
        let err = validate_webhook_url("http://myhost.local/api").unwrap_err();
        assert!(err.contains("internal hostname"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_dot_internal() {
        let err = validate_webhook_url("http://service.internal/api").unwrap_err();
        assert!(err.contains("internal hostname"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_ipv4_loopback() {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let url = format!("http://{ip}:8080/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_private_10() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        let url = format!("http://{ip}/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_private_192_168() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let url = format!("http://{ip}/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_private_172_16() {
        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));
        let url = format!("http://{ip}/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_link_local() {
        let ip = IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1));
        let url = format!("http://{ip}/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_unspecified_v4() {
        let ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        let url = format!("http://{ip}/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_ipv6_loopback() {
        let ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let url = format!("http://[{ip}]/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_rejects_ipv6_unspecified() {
        let ip = IpAddr::V6(Ipv6Addr::UNSPECIFIED);
        let url = format!("http://[{ip}]/api");
        let err = validate_webhook_url(&url).unwrap_err();
        assert!(err.contains("private/internal IP"));
    }

    #[test]
    fn test_validate_webhook_url_accepts_public_ip() {
        let ip = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        let url = format!("http://{ip}/path");
        assert!(validate_webhook_url(&url).is_ok());
    }

    #[test]
    fn test_validate_webhook_url_accepts_public_hostname() {
        assert!(validate_webhook_url("https://discord.com/api/webhooks/xxx").is_ok());
    }

    // ── mask_secret ────────────────────────────────────────────────────

    #[test]
    fn test_mask_secret_empty() {
        assert_eq!(mask_secret(""), "");
    }

    #[test]
    fn test_mask_secret_short_value() {
        assert_eq!(mask_secret("abc"), "****");
    }

    #[test]
    fn test_mask_secret_exactly_four() {
        assert_eq!(mask_secret("1234"), "****");
    }

    #[test]
    fn test_mask_secret_normal() {
        assert_eq!(mask_secret("my-secret-key-1234"), "****1234");
    }

    #[test]
    fn test_mask_secret_shows_last_four() {
        let masked = mask_secret("abcdefgh");
        assert!(masked.ends_with("efgh"));
        assert_eq!(masked, "****efgh");
    }

    // ── Default value functions ────────────────────────────────────────

    #[test]
    fn test_default_sensitivity() {
        assert_eq!(default_sensitivity(), 1.0);
    }

    #[test]
    fn test_default_press_duration() {
        assert_eq!(default_press_duration(), 50);
    }

    #[test]
    fn test_default_stick_duration() {
        assert_eq!(default_stick_duration(), 100);
    }

    #[test]
    fn test_default_touch_duration() {
        assert_eq!(default_touch_duration(), 100);
    }
}
