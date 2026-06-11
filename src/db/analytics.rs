use rusqlite::{Connection, params};
use std::collections::HashMap;
use crate::models::VisitRecord;

// Custom User-Agent parser to avoid bloated dependencies
pub fn parse_ua(ua: &str) -> (String, String, String) {
    let ua_lower = ua.to_lowercase();
    
    let os = if ua_lower.contains("windows") {
        "Windows".to_string()
    } else if ua_lower.contains("macintosh") || ua_lower.contains("mac os x") {
        if ua_lower.contains("iphone") || ua_lower.contains("ipad") {
            "iOS".to_string()
        } else {
            "macOS".to_string()
        }
    } else if ua_lower.contains("android") {
        "Android".to_string()
    } else if ua_lower.contains("linux") {
        "Linux".to_string()
    } else if ua_lower.contains("iphone") || ua_lower.contains("ipad") || ua_lower.contains("ipod") {
        "iOS".to_string()
    } else {
        "Other".to_string()
    };

    let browser = if ua_lower.contains("firefox") {
        "Firefox".to_string()
    } else if ua_lower.contains("opr/") || ua_lower.contains("opera") {
        "Opera".to_string()
    } else if ua_lower.contains("edg/") {
        "Edge".to_string()
    } else if ua_lower.contains("chrome") {
        "Chrome".to_string()
    } else if ua_lower.contains("safari") {
        "Safari".to_string()
    } else {
        "Other".to_string()
    };

    let device = if ua_lower.contains("mobile") || ua_lower.contains("android") || ua_lower.contains("iphone") || ua_lower.contains("ipod") {
        "Mobile".to_string()
    } else if ua_lower.contains("ipad") || ua_lower.contains("tablet") {
        "Tablet".to_string()
    } else {
        "Desktop".to_string()
    };

    (browser, os, device)
}

// Clean referer to domain
pub fn clean_referrer(referer: &str) -> String {
    if referer.is_empty() || referer == "direct" {
        return "Direct".to_string();
    }
    
    if let Ok(url) = reqwest::Url::parse(referer) {
        if let Some(host) = url.host_str() {
            return host.trim_start_matches("www.").to_string();
        }
    }
    
    // Fallback if not a valid URL
    let cleaned = referer.trim_start_matches("https://").trim_start_matches("http://");
    let cleaned = cleaned.split('/').next().unwrap_or("Direct");
    if cleaned.is_empty() {
        "Direct".to_string()
    } else {
        cleaned.trim_start_matches("www.").to_string()
    }
}

pub fn insert_visits_batch(conn: &mut Connection, records: &[VisitRecord]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO visits (id, target_type, target_id, timestamp, ip_address, user_agent, referer, accept_language, country, status_code)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);"
        )?;
        
        for r in records {
            stmt.execute(params![
                r.id,
                r.target_type,
                r.target_id,
                r.timestamp,
                r.ip_address,
                r.user_agent,
                r.referer,
                r.accept_language,
                r.country,
                r.status_code
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn get_total_clicks(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM visits WHERE target_type = 'url';", [], |row| row.get(0))
}

pub fn get_total_page_views(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM visits WHERE target_type = 'page';", [], |row| row.get(0))
}

// Get the date range of visits in the DB
pub fn get_visits_date_range(conn: &Connection) -> rusqlite::Result<Option<(String, String)>> {
    let mut stmt = conn.prepare("SELECT MIN(date(timestamp)), MAX(date(timestamp)) FROM visits;")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let min_date: Option<String> = row.get(0)?;
        let max_date: Option<String> = row.get(1)?;
        if let (Some(min), Some(max)) = (min_date, max_date) {
            return Ok(Some((min, max)));
        }
    }
    Ok(None)
}

// Run aggregation for a specific day
pub fn aggregate_day(conn: &mut Connection, date: &str) -> rusqlite::Result<()> {
    let mut visits = Vec::new();
    {
        // 1. Fetch all visits on that day
        let mut stmt = conn.prepare(
            "SELECT target_type, target_id, user_agent, referer, country, status_code FROM visits WHERE date(timestamp) = ?1;"
        )?;
        
        struct RawVisit {
            target_type: String,
            target_id: String,
            user_agent: String,
            referer: String,
            country: String,
        }
        
        let rows = stmt.query_map(params![date], |row| {
            Ok(RawVisit {
                target_type: row.get(0)?,
                target_id: row.get(1)?,
                user_agent: row.get(2)?,
                referer: row.get(3)?,
                country: row.get(4)?,
            })
        })?;
        
        for r in rows {
            visits.push(r?);
        }
    }

    if visits.is_empty() {
        return Ok(());
    }


    // 2. Compute metrics in-memory
    // Key structure: (target_type, target_id, metric_type, metric_key) -> count
    let mut aggregates: HashMap<(String, String, String, String), i64> = HashMap::new();

    // Also track total per day (all targets combined) using target_id = "all"
    for v in visits {
        let (browser, os, device) = parse_ua(&v.user_agent);
        let referrer = clean_referrer(&v.referer);
        let country = if v.country.is_empty() { "Unknown".to_string() } else { v.country.clone() };

        let targets = vec![
            (v.target_type.clone(), v.target_id.clone()),
            (v.target_type.clone(), "all".to_string()),
        ];

        for (t_type, t_id) in targets {
            // Clicks
            *aggregates.entry((t_type.clone(), t_id.clone(), "clicks".to_string(), "".to_string())).or_insert(0) += 1;
            
            // Country
            *aggregates.entry((t_type.clone(), t_id.clone(), "country".to_string(), country.clone())).or_insert(0) += 1;

            // Browser
            *aggregates.entry((t_type.clone(), t_id.clone(), "browser".to_string(), browser.clone())).or_insert(0) += 1;

            // OS
            *aggregates.entry((t_type.clone(), t_id.clone(), "os".to_string(), os.clone())).or_insert(0) += 1;

            // Device
            *aggregates.entry((t_type.clone(), t_id.clone(), "device".to_string(), device.clone())).or_insert(0) += 1;

            // Referrer
            *aggregates.entry((t_type.clone(), t_id.clone(), "referrer".to_string(), referrer.clone())).or_insert(0) += 1;
        }
    }

    // 3. Save to database in a transaction
    let tx = conn.transaction()?;
    {
        // Delete old aggregates for this day
        tx.execute("DELETE FROM daily_summaries WHERE date = ?1;", params![date])?;

        let mut insert_stmt = tx.prepare(
            "INSERT INTO daily_summaries (date, target_type, target_id, metric_type, metric_key, metric_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);"
        )?;

        for ((t_type, t_id, m_type, m_key), value) in aggregates {
            insert_stmt.execute(params![
                date,
                t_type,
                t_id,
                m_type,
                m_key,
                value
            ])?;
        }
    }
    tx.commit()?;

    // Update monthly and yearly summaries using the daily summaries
    aggregate_month_from_daily(conn, &date[0..7])?;
    aggregate_year_from_daily(conn, &date[0..4])?;

    Ok(())
}

fn aggregate_month_from_daily(conn: &mut Connection, year_month: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        tx.execute("DELETE FROM monthly_summaries WHERE year_month = ?1;", params![year_month])?;
        tx.execute(
            "INSERT INTO monthly_summaries (year_month, target_type, target_id, metric_type, metric_key, metric_value)
             SELECT ?1, target_type, target_id, metric_type, metric_key, SUM(metric_value)
             FROM daily_summaries
             WHERE date LIKE ?2
             GROUP BY target_type, target_id, metric_type, metric_key;",
            params![year_month, format!("{}-%", year_month)],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn aggregate_year_from_daily(conn: &mut Connection, year: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        tx.execute("DELETE FROM yearly_summaries WHERE year = ?1;", params![year])?;
        tx.execute(
            "INSERT INTO yearly_summaries (year, target_type, target_id, metric_type, metric_key, metric_value)
             SELECT ?1, target_type, target_id, metric_type, metric_key, SUM(metric_value)
             FROM daily_summaries
             WHERE date LIKE ?2
             GROUP BY target_type, target_id, metric_type, metric_key;",
            params![year, format!("{}-%", year)],
        )?;
    }
    tx.commit()?;
    Ok(())
}

// Clean old raw visit records
pub fn retention_cleanup(conn: &Connection, retention_days: i64) -> rusqlite::Result<usize> {
    let limit_date = chrono::Utc::now() - chrono::Duration::days(retention_days);
    let limit_str = limit_date.to_rfc3339();
    let count = conn.execute("DELETE FROM visits WHERE timestamp < ?1;", params![limit_str])?;
    Ok(count)
}

// --- Query functions for Dashboard & API ---

pub fn get_clicks_trend(
    conn: &Connection,
    target_type: &str,
    target_id: &str,
    limit_days: i64,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let limit_date = (chrono::Utc::now() - chrono::Duration::days(limit_days)).format("%Y-%m-%d").to_string();
    
    let mut stmt = conn.prepare(
        "SELECT date, SUM(metric_value) FROM daily_summaries 
         WHERE target_type = ?1 AND target_id = ?2 AND metric_type = 'clicks' AND date >= ?3
         GROUP BY date ORDER BY date ASC;"
    )?;
    
    let rows = stmt.query_map(params![target_type, target_id, limit_date], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let mut res = Vec::new();
    for r in rows {
        res.push(r?);
    }
    Ok(res)
}

// Fallback to query raw visits table if daily summaries are not aggregated yet
pub fn get_clicks_trend_raw(
    conn: &Connection,
    target_type: &str,
    target_id: &str,
    limit_days: i64,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let limit_date = (chrono::Utc::now() - chrono::Duration::days(limit_days)).to_rfc3339();
    
    let mut stmt = conn.prepare(
        "SELECT date(timestamp) as d, COUNT(*) FROM visits 
         WHERE target_type = ?1 AND target_id = ?2 AND timestamp >= ?3
         GROUP BY d ORDER BY d ASC;"
    )?;
    
    let rows = stmt.query_map(params![target_type, target_id, limit_date], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let mut res = Vec::new();
    for r in rows {
        res.push(r?);
    }
    Ok(res)
}

pub fn get_hourly_trend_raw(
    conn: &Connection,
    target_type: &str,
    target_id: &str,
    limit_days: i64,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let limit_date = (chrono::Utc::now() - chrono::Duration::days(limit_days)).to_rfc3339();
    
    // SQLite strftime('%H', timestamp) extracts the hour
    let mut stmt = conn.prepare(
        "SELECT strftime('%H', timestamp) as h, COUNT(*) FROM visits 
         WHERE target_type = ?1 AND target_id = ?2 AND timestamp >= ?3
         GROUP BY h ORDER BY h ASC;"
    )?;
    
    let rows = stmt.query_map(params![target_type, target_id, limit_date], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let mut res = Vec::new();
    for r in rows {
        res.push(r?);
    }
    Ok(res)
}

pub fn get_metric_rankings(
    conn: &Connection,
    target_type: &str,
    target_id: &str,
    metric_type: &str,
    limit: i64,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT metric_key, SUM(metric_value) as val FROM daily_summaries 
         WHERE target_type = ?1 AND target_id = ?2 AND metric_type = ?3
         GROUP BY metric_key ORDER BY val DESC LIMIT ?4;"
    )?;
    
    let rows = stmt.query_map(params![target_type, target_id, metric_type, limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let mut res = Vec::new();
    for r in rows {
        res.push(r?);
    }
    Ok(res)
}

pub fn get_metric_rankings_raw(
    conn: &Connection,
    target_type: &str,
    target_id: &str,
    metric_type: &str,
    limit: i64,
) -> rusqlite::Result<Vec<(String, i64)>> {
    // Falls back to direct query on visits
    let mut res = Vec::new();
    
    match metric_type {
        "country" => {
            let mut stmt = conn.prepare(
                "SELECT country, COUNT(*) as c FROM visits 
                 WHERE target_type = ?1 AND target_id = ?2 
                 GROUP BY country ORDER BY c DESC LIMIT ?3;"
            )?;
            let rows = stmt.query_map(params![target_type, target_id, limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for r in rows { res.push(r?); }
        }
        "referrer" => {
            let mut stmt = conn.prepare(
                "SELECT referer, COUNT(*) as c FROM visits 
                 WHERE target_type = ?1 AND target_id = ?2 
                 GROUP BY referer ORDER BY c DESC LIMIT ?3;"
            )?;
            let rows = stmt.query_map(params![target_type, target_id, limit], |row| {
                let raw_ref: String = row.get(0)?;
                Ok((clean_referrer(&raw_ref), row.get::<_, i64>(1)?))
            })?;
            
            // Re-aggregate because clean_referrer might group different referrers
            let mut grouped: HashMap<String, i64> = HashMap::new();
            for r in rows {
                let (k, v) = r?;
                *grouped.entry(k).or_insert(0) += v;
            }
            res = grouped.into_iter().collect();
            res.sort_by_key(|b| std::cmp::Reverse(b.1));
            res.truncate(limit as usize);
        }
        "browser" | "os" | "device" => {
            let mut stmt = conn.prepare(
                "SELECT user_agent, COUNT(*) as c FROM visits 
                 WHERE target_type = ?1 AND target_id = ?2 
                 GROUP BY user_agent;"
            )?;
            let rows = stmt.query_map(params![target_type, target_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            
            let mut grouped: HashMap<String, i64> = HashMap::new();
            for r in rows {
                let (ua, count) = r?;
                let (b, o, d) = parse_ua(&ua);
                let key = match metric_type {
                    "browser" => b,
                    "os" => o,
                    _ => d,
                };
                *grouped.entry(key).or_insert(0) += count;
            }
            res = grouped.into_iter().collect();
            res.sort_by_key(|b| std::cmp::Reverse(b.1));
            res.truncate(limit as usize);
        }
        _ => {}
    }
    
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ua_browsers() {
        let firefox_linux = "Mozilla/5.0 (X11; Linux x86_64; rv:109.0) Gecko/20100101 Firefox/119.0";
        let chrome_win = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        let safari_mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Safari/605.1.15";
        let android_phone = "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Mobile Safari/537.36";

        assert_eq!(parse_ua(firefox_linux), ("Firefox".to_string(), "Linux".to_string(), "Desktop".to_string()));
        assert_eq!(parse_ua(chrome_win), ("Chrome".to_string(), "Windows".to_string(), "Desktop".to_string()));
        assert_eq!(parse_ua(safari_mac), ("Safari".to_string(), "macOS".to_string(), "Desktop".to_string()));
        assert_eq!(parse_ua(android_phone), ("Chrome".to_string(), "Android".to_string(), "Mobile".to_string()));
    }

    #[test]
    fn test_clean_referrer() {
        assert_eq!(clean_referrer("direct"), "Direct");
        assert_eq!(clean_referrer(""), "Direct");
        assert_eq!(clean_referrer("https://github.com/rust-lang/rust"), "github.com");
        assert_eq!(clean_referrer("http://www.google.com/search?q=rust"), "google.com");
        assert_eq!(clean_referrer("reddit.com/r/rust"), "reddit.com");
    }
}

