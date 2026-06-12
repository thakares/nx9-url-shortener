use bzod::db::migrations::{run_migrations, ANALYTICS_MIGRATIONS};
use bzod::db::qr::{
    get_qr_code_style, get_qr_scan_count, get_qr_stats_for_url, log_qr_access, upsert_qr_code,
};
use rusqlite::Connection;

#[test]
fn test_qr_service_png_and_svg() {
    let url = "https://example.com/some/path";
    let png = bzod::services::qr::generate_qr_png(url, 256).unwrap();
    assert!(!png.is_empty());
    assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']); // PNG magic bytes

    let svg = bzod::services::qr::generate_qr_svg(url).unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("</svg>"));
}

#[test]
fn test_qr_access_logging() {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "analytics", ANALYTICS_MIGRATIONS, None).unwrap();

    let url_id = "test-url-uuid-123";
    log_qr_access(&conn, url_id, Some("127.0.0.1"), Some("TestBrowser/1.0")).unwrap();
    log_qr_access(&conn, url_id, None, None).unwrap();

    let count = get_qr_scan_count(&conn, url_id).unwrap();
    assert_eq!(count, 2);

    let stats = get_qr_stats_for_url(&conn, url_id).unwrap();
    assert_eq!(stats.len(), 2);
    assert_eq!(stats[0].1, "");
    assert_eq!(stats[1].1, "127.0.0.1");
}

#[test]
fn test_qr_code_styling() {
    let mut conn = Connection::open_in_memory().unwrap();
    // Run content migrations because qr_codes table is in content.db
    run_migrations(
        &mut conn,
        "content",
        bzod::db::migrations::CONTENT_MIGRATIONS,
        None,
    )
    .unwrap();

    // Create a dummy URL first to avoid FOREIGN KEY failure
    let url = bzod::db::content::create_url_extended(
        &conn,
        "qrstyle",
        "https://example.com",
        None,
        None,
        &[],
        None,
        None,
        None,
    )
    .unwrap();

    // Default style when not configured
    let style = get_qr_code_style(&conn, &url.id).unwrap();
    assert_eq!(style, "default");

    // Upsert a style
    upsert_qr_code(&conn, &url.id, "fancy-blue").unwrap();
    let style = get_qr_code_style(&conn, &url.id).unwrap();
    assert_eq!(style, "fancy-blue");

    // Update the style
    upsert_qr_code(&conn, &url.id, "sleek-dark").unwrap();
    let style = get_qr_code_style(&conn, &url.id).unwrap();
    assert_eq!(style, "sleek-dark");
}
