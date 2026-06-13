use axum::http::StatusCode;
use bzod::web::pages::root_landing;
use std::fs;

#[tokio::test]
async fn test_root_landing_page() {
    // --- Test case 1: Successful serving ---
    // Read expected content
    let expected_content =
        fs::read_to_string("www/index.html").expect("www/index.html must exist for test");

    let response = root_landing().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("text/html"));

    // Convert response body to bytes using axum::body::to_bytes
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
    assert_eq!(body_str, expected_content);

    // --- Test case 2: File Not Found fallback ---
    // Temporarily rename www/index.html to simulate file not found
    let orig_path = "www/index.html";
    let temp_path = "www/index.html.tmp_test_bak";
    fs::rename(orig_path, temp_path).unwrap();

    let response_res = tokio::spawn(async move { root_landing().await }).await;

    // Restore index.html immediately in case of panic/error
    let rename_res = fs::rename(temp_path, orig_path);

    // Now verify the response status
    let response = response_res.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    rename_res.unwrap();
}
