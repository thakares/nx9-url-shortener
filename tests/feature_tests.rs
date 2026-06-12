use bzod::db::audit_events::{list_audit_events, write_audit_event};
use bzod::db::content::{
    create_url_extended, expire_urls, get_url_by_id, increment_access_count, remove_url_password,
    set_url_password,
};
use bzod::db::migrations::{run_migrations, CONTENT_MIGRATIONS, SYSTEM_MIGRATIONS};
use bzod::db::preview::{delete_preview, get_preview, upsert_preview};
use chrono::Utc;
use rusqlite::Connection;

fn setup_content_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "content", CONTENT_MIGRATIONS, None).unwrap();
    conn
}

fn setup_system_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "system", SYSTEM_MIGRATIONS, None).unwrap();
    conn
}

#[test]
fn test_expiring_links() {
    let conn = setup_content_db();

    // Create an expired URL
    let past = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let url_expired = create_url_extended(
        &conn,
        "exp001",
        "https://expired.com",
        Some("Expired Link"),
        None,
        &[],
        Some(&past),
        None,
        None,
    )
    .unwrap();

    // Create a future URL (not expired)
    let future = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let url_active = create_url_extended(
        &conn,
        "act001",
        "https://active.com",
        Some("Active Link"),
        None,
        &[],
        Some(&future),
        None,
        None,
    )
    .unwrap();

    // Verify initial state
    assert!(!url_expired.expired);
    assert!(!url_active.expired);

    // Run expiration logic
    let expired_count = expire_urls(&conn).unwrap();
    assert_eq!(expired_count, 1);

    // Verify after expiration
    let url_expired_after = get_url_by_id(&conn, &url_expired.id).unwrap().unwrap();
    let url_active_after = get_url_by_id(&conn, &url_active.id).unwrap().unwrap();

    assert!(url_expired_after.expired);
    assert!(!url_active_after.expired);
}

#[test]
fn test_password_protected_links() {
    let conn = setup_content_db();

    let url = create_url_extended(
        &conn,
        "pwd001",
        "https://protected.com",
        None,
        None,
        &[],
        None,
        None,
        None,
    )
    .unwrap();

    assert!(!url.is_password_protected());

    // Set password hash
    let hash = "fake_argon_hash";
    let set_ok = set_url_password(&conn, &url.id, hash).unwrap();
    assert!(set_ok);

    let url_updated = get_url_by_id(&conn, &url.id).unwrap().unwrap();
    assert!(url_updated.is_password_protected());
    assert_eq!(url_updated.password_hash.as_deref(), Some(hash));

    // Remove password
    let remove_ok = remove_url_password(&conn, &url.id).unwrap();
    assert!(remove_ok);

    let url_removed = get_url_by_id(&conn, &url.id).unwrap().unwrap();
    assert!(!url_removed.is_password_protected());
}

#[test]
fn test_one_time_links() {
    let conn = setup_content_db();

    let url = create_url_extended(
        &conn,
        "one001",
        "https://onetime.com",
        None,
        None,
        &[],
        None,
        None,
        Some(2), // Max access count = 2
    )
    .unwrap();

    assert!(!url.is_access_exhausted());

    // 1st click
    let clicks = increment_access_count(&conn, &url.id).unwrap();
    assert_eq!(clicks, 1);

    let url_refetched = get_url_by_id(&conn, &url.id).unwrap().unwrap();
    assert!(!url_refetched.is_access_exhausted());

    // 2nd click
    let clicks2 = increment_access_count(&conn, &url.id).unwrap();
    assert_eq!(clicks2, 2);

    let url_refetched2 = get_url_by_id(&conn, &url.id).unwrap().unwrap();
    assert!(url_refetched2.is_access_exhausted());
}

#[test]
fn test_link_previews() {
    let conn = setup_content_db();

    let url = create_url_extended(
        &conn,
        "prv001",
        "https://previewed.com",
        None,
        None,
        &[],
        None,
        None,
        None,
    )
    .unwrap();

    // Initial check: no preview
    let prev_init = get_preview(&conn, &url.id).unwrap();
    assert!(prev_init.is_none());

    // Create preview
    let prev = upsert_preview(
        &conn,
        &url.id,
        Some("Sample Page"),
        Some("Sample Description"),
        Some("https://logo.png"),
        Some("Proceed"),
    )
    .unwrap();

    assert_eq!(prev.title.as_deref(), Some("Sample Page"));
    assert_eq!(prev.button_text, "Proceed");

    // Get preview
    let prev_get = get_preview(&conn, &url.id).unwrap().unwrap();
    assert_eq!(prev_get.description.as_deref(), Some("Sample Description"));

    // Update preview
    let prev_updated =
        upsert_preview(&conn, &url.id, Some("Updated Page"), None, None, None).unwrap();
    assert_eq!(prev_updated.title.as_deref(), Some("Updated Page"));
    assert_eq!(prev_updated.button_text, "Continue"); // defaults

    // Delete preview
    let del_ok = delete_preview(&conn, &url.id).unwrap();
    assert!(del_ok);

    let prev_final = get_preview(&conn, &url.id).unwrap();
    assert!(prev_final.is_none());
}

#[test]
fn test_system_audit_events() {
    let conn = setup_system_db();

    // Initial check
    let events_init = list_audit_events(&conn, 100, 0, None, None).unwrap();
    assert!(events_init.is_empty());

    // Write events
    write_audit_event(
        &conn,
        "admin",
        "URL_CREATION",
        "url",
        "url-uuid-1",
        Some("metadata-1"),
    )
    .unwrap();
    write_audit_event(&conn, "api-user", "URL_UPDATE", "url", "url-uuid-2", None).unwrap();

    // List events
    let events = list_audit_events(&conn, 100, 0, None, None).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].actor, "api-user"); // ordered desc by timestamp
    assert_eq!(events[1].actor, "admin");

    // Filter by actor
    let events_filtered = list_audit_events(&conn, 100, 0, Some("admin"), None).unwrap();
    assert_eq!(events_filtered.len(), 1);
    assert_eq!(events_filtered[0].action, "URL_CREATION");
}
