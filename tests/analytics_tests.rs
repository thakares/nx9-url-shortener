use bzod::db::analytics::{
    get_monthly_clicks_trend, get_target_unique_visitors, get_target_visit_count,
    insert_visits_batch,
};
use bzod::db::content::{create_url_extended, get_url_count_by_tag, get_url_counts, list_urls};
use bzod::db::migrations::{run_migrations, ANALYTICS_MIGRATIONS, CONTENT_MIGRATIONS};
use bzod::models::VisitRecord;
use chrono::Utc;
use rusqlite::Connection;

fn setup_content_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "content", CONTENT_MIGRATIONS, None).unwrap();
    conn
}

fn setup_analytics_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "analytics", ANALYTICS_MIGRATIONS, None).unwrap();
    conn
}

#[test]
fn test_get_url_count_by_tag() {
    let conn = setup_content_db();

    // Verify tag count on empty db
    let count = get_url_count_by_tag(&conn, "promo").unwrap();
    assert_eq!(count, 0);

    // Create a URL and associate tags
    create_url_extended(
        &conn,
        "code1",
        "https://example.com/1",
        None,
        None,
        &["promo".to_string(), "tech".to_string()],
        None,
        None,
        None,
    )
    .unwrap();

    create_url_extended(
        &conn,
        "code2",
        "https://example.com/2",
        None,
        None,
        &["promo".to_string()],
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(get_url_count_by_tag(&conn, "promo").unwrap(), 2);
    assert_eq!(get_url_count_by_tag(&conn, "tech").unwrap(), 1);
    assert_eq!(get_url_count_by_tag(&conn, "other").unwrap(), 0);
}

#[test]
fn test_pagination_limits_and_offset() {
    let conn = setup_content_db();

    // Create 27 URLs to test page pagination limits of PAGE_SIZE = 25
    for i in 0..27 {
        create_url_extended(
            &conn,
            &format!("cd{:03}", i),
            &format!("https://example.com/{}", i),
            None,
            None,
            &[],
            None,
            None,
            None,
        )
        .unwrap();
    }

    let (total, _, _) = get_url_counts(&conn).unwrap();
    assert_eq!(total, 27);

    // Page 1 (limit 25, offset 0)
    let page1_urls = list_urls(&conn, 25, 0, None).unwrap();
    assert_eq!(page1_urls.len(), 25);

    // Page 2 (limit 25, offset 25)
    let page2_urls = list_urls(&conn, 25, 25, None).unwrap();
    assert_eq!(page2_urls.len(), 2);
}

#[test]
fn test_target_analytics_queries() {
    let mut conn = setup_analytics_db();
    let target_uuid = "target-uuid-123456";

    // 1. Visit Count and Unique Visitors check on empty db
    assert_eq!(
        get_target_visit_count(&conn, "url", target_uuid).unwrap(),
        0
    );
    assert_eq!(
        get_target_unique_visitors(&conn, "url", target_uuid).unwrap(),
        0
    );

    // 2. Insert some visit records
    let now = Utc::now();
    let records = vec![
        VisitRecord {
            id: "visit-1".to_string(),
            target_type: "url".to_string(),
            target_id: target_uuid.to_string(),
            timestamp: now.to_rfc3339(),
            ip_address: "1.1.1.1".to_string(),
            user_agent: "Mozilla".to_string(),
            referer: "direct".to_string(),
            accept_language: "en".to_string(),
            country: "US".to_string(),
            status_code: 200,
            owner_tenant_id: None,
            owner_user_id: None,
        },
        VisitRecord {
            id: "visit-2".to_string(),
            target_type: "url".to_string(),
            target_id: target_uuid.to_string(),
            timestamp: now.to_rfc3339(),
            ip_address: "1.1.1.1".to_string(), // same visitor
            user_agent: "Mozilla".to_string(),
            referer: "direct".to_string(),
            accept_language: "en".to_string(),
            country: "US".to_string(),
            status_code: 200,
            owner_tenant_id: None,
            owner_user_id: None,
        },
        VisitRecord {
            id: "visit-3".to_string(),
            target_type: "url".to_string(),
            target_id: target_uuid.to_string(),
            timestamp: now.to_rfc3339(),
            ip_address: "2.2.2.2".to_string(), // different visitor
            user_agent: "Mozilla".to_string(),
            referer: "direct".to_string(),
            accept_language: "en".to_string(),
            country: "US".to_string(),
            status_code: 200,
            owner_tenant_id: None,
            owner_user_id: None,
        },
    ];

    insert_visits_batch(&mut conn, &records).unwrap();

    // 3. Verify counts
    assert_eq!(
        get_target_visit_count(&conn, "url", target_uuid).unwrap(),
        3
    );
    assert_eq!(
        get_target_unique_visitors(&conn, "url", target_uuid).unwrap(),
        2
    );

    // 4. Verify monthly clicks trend fallback
    let monthly_trend = get_monthly_clicks_trend(&conn, "url", target_uuid, 12).unwrap();
    assert_eq!(monthly_trend.len(), 1);
    assert_eq!(monthly_trend[0].1, 3); // 3 clicks total
}
