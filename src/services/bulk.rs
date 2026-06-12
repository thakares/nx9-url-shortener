use crate::models::Url;
use crate::services::qr::{generate_qr_png, generate_qr_svg};
use std::io::Write;
use zip::write::FileOptions;
use zip::ZipWriter;

/// Export QR codes for the given URLs as a ZIP file.
/// `format` can be "png" or "svg".
/// `base_url` is used to build the full short URL encoded in the QR.
pub fn export_qr_zip(
    urls: &[Url],
    format: &str,
    base_url: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options =
            FileOptions::<()>::default().compression_method(zip::CompressionMethod::Deflated);

        let mut csv_content = String::from("code,destination,qr_filename\n");

        for url in urls {
            let full_url = format!("{}/{}", base_url.trim_end_matches('/'), url.code);
            let ext = if format == "svg" { "svg" } else { "png" };
            let filename = format!("{}.{}", url.code, ext);

            let qr_data = if format == "svg" {
                generate_qr_svg(&full_url)?.into_bytes()
            } else {
                generate_qr_png(&full_url, 256)?
            };

            zip.start_file(&filename, options)?;
            zip.write_all(&qr_data)?;

            // Escape quotes in destination for CSV formatting
            let escaped_dest = url.destination.replace('"', "\"\"");
            csv_content.push_str(&format!("{},\"{}\",{}\n", url.code, escaped_dest, filename));
        }

        zip.start_file("manifest.csv", options)?;
        zip.write_all(csv_content.as_bytes())?;

        zip.finish()?;
    }

    Ok(buf)
}
