use std::net::IpAddr;
use std::path::PathBuf;

use pokecon_core::cv::camera::{Frame, PixelFormat};
use pokecon_core::serial::SendFormat;
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

// ── Helper: Convert send format to default serial row string ───────────────────

pub fn format_default_row(
    fmt: &SendFormat,
    l_stick_changed: bool,
    r_stick_changed: bool,
) -> String {
    let mut send_btn = (fmt.btn as u32) << 2;
    if l_stick_changed {
        send_btn |= 0x2;
    }
    if r_stick_changed {
        send_btn |= 0x1;
    }

    let mut result = format!("{send_btn:#08x}");
    result.push(' ');
    result.push_str(&fmt.hat.to_string());

    if l_stick_changed {
        result.push_str(&format!(" {:x} {:x}", fmt.lx, fmt.ly));
    }
    if r_stick_changed {
        result.push_str(&format!(" {:x} {:x}", fmt.rx, fmt.ry));
    }

    result
}

/// Save a camera frame as a JPEG file at the given path.
pub fn save_frame_as_jpeg(frame: &Frame, path: &PathBuf) -> Result<(), String> {
    let (data, width, height) = match frame.format {
        PixelFormat::Rgb => (frame.data.clone(), frame.width, frame.height),
        PixelFormat::Bgr => {
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(3)
                .flat_map(|c| [c[2], c[1], c[0]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Rgba => {
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(4)
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Gray => {
            let rgb: Vec<u8> = frame.data.iter().flat_map(|&g| [g, g, g]).collect();
            (rgb, frame.width, frame.height)
        }
    };

    let mut file =
        std::fs::File::create(path).map_err(|e| format!("Failed to create file: {e}"))?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut file);
    encoder
        .encode(&data, width, height, image::ColorType::Rgb8)
        .map_err(|e| format!("JPEG encode error: {e}"))?;

    Ok(())
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
