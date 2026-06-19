use bzod::db::analytics::{
    clean_referrer, get_target_visit_total_filtered, get_target_visits_all_in_memory,
    get_target_visits_paginated, get_visits_schema_columns, insert_visits_batch, parse_ua,
};
use bzod::db::migrations::{run_migrations, ANALYTICS_MIGRATIONS};
use bzod::models::VisitRecord;
use rusqlite::Connection;

fn setup_analytics_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn, "analytics", ANALYTICS_MIGRATIONS, None).unwrap();
    conn
}

#[test]
fn test_advanced_analytics_schema_columns() {
    let conn = setup_analytics_db();
    let columns = get_visits_schema_columns(&conn).unwrap();
    assert!(columns.contains("id"));
    assert!(columns.contains("target_type"));
    assert!(columns.contains("target_id"));
    assert!(columns.contains("timestamp"));
    assert!(columns.contains("ip_address"));
    assert!(columns.contains("user_agent"));
    assert!(columns.contains("referer"));
    assert!(columns.contains("country"));
    assert!(columns.contains("status_code"));
    // Verify UTM columns don't exist by default
    assert!(!columns.contains("utm_source"));
    assert!(!columns.contains("utm_campaign"));
}

#[test]
fn test_advanced_analytics_pagination_and_sorting() {
    let mut conn = setup_analytics_db();
    let target_uuid = "target-123";

    // Insert 55 visits to test pagination bounds (ANALYTICS_PAGE_SIZE = 50)
    let mut records = Vec::new();
    for i in 1..=55 {
        // Stagger timestamps so they are ordered deterministically
        let time_str = format!("2026-06-17T12:00:{:02}Z", i);
        records.push(VisitRecord {
            id: format!("visit-{}", i),
            target_type: "url".to_string(),
            target_id: target_uuid.to_string(),
            timestamp: time_str,
            ip_address: "1.1.1.1".to_string(),
            user_agent: "Mozilla".to_string(),
            referer: "direct".to_string(),
            accept_language: "en".to_string(),
            country: "US".to_string(),
            status_code: 200,
            owner_user_id: None,
        });
    }
    insert_visits_batch(&mut conn, &records).unwrap();

    // Verify total count
    let total = get_target_visit_total_filtered(&conn, "url", target_uuid, None, None).unwrap();
    assert_eq!(total, 55);

    // Page 1 (limit 50, offset 0)
    let page1 = get_target_visits_paginated(&conn, "url", target_uuid, 50, 0, None, None).unwrap();
    assert_eq!(page1.len(), 50);
    // Ordered DESC: first record returned should be the latest (visit-55)
    assert_eq!(page1[0].id, "visit-55");
    assert_eq!(page1[49].id, "visit-6");

    // Page 2 (limit 50, offset 50)
    let page2 = get_target_visits_paginated(&conn, "url", target_uuid, 50, 50, None, None).unwrap();
    assert_eq!(page2.len(), 5);
    assert_eq!(page2[0].id, "visit-5");
    assert_eq!(page2[4].id, "visit-1");
}

#[test]
fn test_advanced_analytics_date_filtering() {
    let mut conn = setup_analytics_db();
    let target_uuid = "target-456";

    // Insert records across different days
    let records = vec![
        VisitRecord {
            id: "v1".to_string(),
            target_type: "url".to_string(),
            target_id: target_uuid.to_string(),
            timestamp: "2026-06-01T10:00:00Z".to_string(),
            ip_address: "1.1.1.1".to_string(),
            user_agent: "Mozilla".to_string(),
            referer: "direct".to_string(),
            accept_language: "en".to_string(),
            country: "US".to_string(),
            status_code: 200,
            owner_user_id: None,
        },
        VisitRecord {
            id: "v2".to_string(),
            target_type: "url".to_string(),
            target_id: target_uuid.to_string(),
            timestamp: "2026-06-15T10:00:00Z".to_string(),
            ip_address: "1.1.1.1".to_string(),
            user_agent: "Mozilla".to_string(),
            referer: "direct".to_string(),
            accept_language: "en".to_string(),
            country: "US".to_string(),
            status_code: 200,
            owner_user_id: None,
        },
        VisitRecord {
            id: "v3".to_string(),
            target_type: "url".to_string(),
            target_id: target_uuid.to_string(),
            timestamp: "2026-06-30T10:00:00Z".to_string(),
            ip_address: "1.1.1.1".to_string(),
            user_agent: "Mozilla".to_string(),
            referer: "direct".to_string(),
            accept_language: "en".to_string(),
            country: "US".to_string(),
            status_code: 200,
            owner_user_id: None,
        },
    ];
    insert_visits_batch(&mut conn, &records).unwrap();

    // 1. Filter with date_from = "2026-06-15" (inclusive) -> should get v2 and v3
    let count1 =
        get_target_visit_total_filtered(&conn, "url", target_uuid, Some("2026-06-15"), None)
            .unwrap();
    assert_eq!(count1, 2);
    let results1 =
        get_target_visits_all_in_memory(&conn, "url", target_uuid, Some("2026-06-15"), None)
            .unwrap();
    assert_eq!(results1.len(), 2);
    assert_eq!(results1[0].id, "v3"); // DESC sorting
    assert_eq!(results1[1].id, "v2");

    // 2. Filter with date_to = "2026-06-15" (inclusive) -> should get v1 and v2
    let count2 =
        get_target_visit_total_filtered(&conn, "url", target_uuid, None, Some("2026-06-15"))
            .unwrap();
    assert_eq!(count2, 2);
    let results2 =
        get_target_visits_all_in_memory(&conn, "url", target_uuid, None, Some("2026-06-15"))
            .unwrap();
    assert_eq!(results2.len(), 2);
    assert_eq!(results2[0].id, "v2");
    assert_eq!(results2[1].id, "v1");

    // 3. Filter with both date_from and date_to = "2026-06-15"
    let count3 = get_target_visit_total_filtered(
        &conn,
        "url",
        target_uuid,
        Some("2026-06-15"),
        Some("2026-06-15"),
    )
    .unwrap();
    assert_eq!(count3, 1);
    let results3 = get_target_visits_all_in_memory(
        &conn,
        "url",
        target_uuid,
        Some("2026-06-15"),
        Some("2026-06-15"),
    )
    .unwrap();
    assert_eq!(results3.len(), 1);
    assert_eq!(results3[0].id, "v2");
}

#[test]
fn test_parse_ua_and_referrer_helpers() {
    let (browser, os, device) = parse_ua("Mozilla/5.0 (iPhone; CPU iPhone OS 16_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.5 Mobile/15E148 Safari/605.1.15");
    assert_eq!(browser, "Safari");
    assert_eq!(os, "iOS");
    assert_eq!(device, "Mobile");

    let referrer = clean_referrer("https://news.ycombinator.com/item?id=12345");
    assert_eq!(referrer, "news.ycombinator.com");
}
