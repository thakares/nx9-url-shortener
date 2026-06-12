use crate::models::AuditEvent;
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Write an audit event to the system.db audit_events table.
pub fn write_audit_event(
    conn: &Connection,
    actor: &str,
    action: &str,
    object_type: &str,
    object_id: &str,
    metadata: Option<&str>,
) -> rusqlite::Result<()> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO audit_events (id, actor, action, object_type, object_id, timestamp, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
        params![id, actor, action, object_type, object_id, now, metadata],
    )?;
    Ok(())
}

/// List audit events with optional filtering by actor or action.
pub fn list_audit_events(
    conn: &Connection,
    limit: i64,
    offset: i64,
    actor_filter: Option<&str>,
    action_filter: Option<&str>,
) -> rusqlite::Result<Vec<AuditEvent>> {
    let mut events = Vec::new();

    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match (actor_filter, action_filter) {
        (Some(actor), Some(action)) => (
            "SELECT id, actor, action, object_type, object_id, timestamp, metadata FROM audit_events WHERE actor = ?1 AND action = ?2 ORDER BY timestamp DESC LIMIT ?3 OFFSET ?4;".to_string(),
            vec![Box::new(actor.to_string()), Box::new(action.to_string()), Box::new(limit), Box::new(offset)],
        ),
        (Some(actor), None) => (
            "SELECT id, actor, action, object_type, object_id, timestamp, metadata FROM audit_events WHERE actor = ?1 ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3;".to_string(),
            vec![Box::new(actor.to_string()), Box::new(limit), Box::new(offset)],
        ),
        (None, Some(action)) => (
            "SELECT id, actor, action, object_type, object_id, timestamp, metadata FROM audit_events WHERE action = ?1 ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3;".to_string(),
            vec![Box::new(action.to_string()), Box::new(limit), Box::new(offset)],
        ),
        (None, None) => (
            "SELECT id, actor, action, object_type, object_id, timestamp, metadata FROM audit_events ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2;".to_string(),
            vec![Box::new(limit), Box::new(offset)],
        ),
    };

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(AuditEvent {
            id: row.get(0)?,
            actor: row.get(1)?,
            action: row.get(2)?,
            object_type: row.get(3)?,
            object_id: row.get(4)?,
            timestamp: row.get(5)?,
            metadata: row.get(6)?,
        })
    })?;

    for event in rows {
        events.push(event?);
    }
    Ok(events)
}
