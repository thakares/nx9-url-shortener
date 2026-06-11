use rusqlite::{Connection, params};
use std::collections::HashMap;
use crate::db::analytics::{parse_ua, clean_referrer};

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

pub fn aggregate_month_from_daily(conn: &mut Connection, year_month: &str) -> rusqlite::Result<()> {
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

pub fn aggregate_year_from_daily(conn: &mut Connection, year: &str) -> rusqlite::Result<()> {
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
