//! QR code generation service.
//!
//! Generates QR codes on-demand as PNG or SVG. No files are stored on disk.

use image::Luma;
use qrcode::QrCode;
use std::io::Cursor;

/// Generate a QR code as a PNG byte vector.
///
/// The `url` is encoded into the QR matrix. The `size` parameter controls
/// the pixel dimensions of the output image (default: 256).
pub fn generate_qr_png(url: &str, size: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let code = QrCode::new(url.as_bytes())?;
    let image = code.render::<Luma<u8>>().min_dimensions(size, size).build();

    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    image.write_to(&mut cursor, image::ImageFormat::Png)?;
    Ok(buf)
}

/// Generate a QR code as an SVG string.
pub fn generate_qr_svg(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let code = QrCode::new(url.as_bytes())?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(256, 256)
        .build();
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_png_generation() {
        let png = generate_qr_png("https://bzo.in/abc123", 256).unwrap();
        assert!(!png.is_empty());
        // PNG magic bytes
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn test_qr_svg_generation() {
        let svg = generate_qr_svg("https://bzo.in/abc123").unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn test_qr_long_url() {
        let long_url = format!("https://example.com/{}", "a".repeat(500));
        let png = generate_qr_png(&long_url, 512).unwrap();
        assert!(!png.is_empty());
    }
}
