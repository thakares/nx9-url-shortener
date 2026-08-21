//! Independently versioned v0.8 schemas.
//!
//! Phase 1 creates the frozen slug databases. Live slug allocate/lookup still
//! uses `system.db.global_slugs` until Phase 4 moves ownership.

use super::migrations::Migration;

/// `slugs/global_urls.db` — globally unique URL slugs.
///
/// `owner_user_id` remains INTEGER to match v0.7 `users.id`. Phase 3 will
/// migrate it to the 12-hex user id.
pub const GLOBAL_URLS_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
    CREATE TABLE IF NOT EXISTS global_urls (
        slug TEXT PRIMARY KEY,
        owner_tenant_id TEXT NOT NULL,
        target_id TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        status TEXT NOT NULL,
        retired_at TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_global_urls_tenant ON global_urls(owner_tenant_id);
    CREATE INDEX IF NOT EXISTS idx_global_urls_status ON global_urls(status);
    "#,
    },
    Migration {
        version: 2,
        name: "tenant_id_column",
        sql: r#"
    ALTER TABLE global_urls ADD COLUMN owner_tenant_id TEXT;
    CREATE INDEX IF NOT EXISTS idx_global_urls_tenant ON global_urls(owner_tenant_id);
    "#,
    },
];

/// `slugs/global_landing_pages.db` — globally unique landing-page slugs.
pub const GLOBAL_LANDING_PAGES_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
    CREATE TABLE IF NOT EXISTS global_landing_pages (
        slug TEXT PRIMARY KEY,
        owner_tenant_id TEXT NOT NULL,
        target_id TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        status TEXT NOT NULL,
        retired_at TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_global_landing_pages_tenant ON global_landing_pages(owner_tenant_id);
    CREATE INDEX IF NOT EXISTS idx_global_landing_pages_status ON global_landing_pages(status);
    "#,
    },
    Migration {
        version: 2,
        name: "tenant_id_column",
        sql: r#"
    ALTER TABLE global_landing_pages ADD COLUMN owner_tenant_id TEXT;
    CREATE INDEX IF NOT EXISTS idx_global_landing_pages_tenant ON global_landing_pages(owner_tenant_id);
    "#,
    },
];

/// `slugs/reserved.db` — system route / reserved names that must never allocate.
pub const RESERVED_SLUGS_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_schema",
    sql: r#"
    CREATE TABLE IF NOT EXISTS reserved_slugs (
        slug TEXT PRIMARY KEY,
        reason TEXT
    );
    "#,
}];

/// Allowed statuses for the v0.8 slug databases.
///
/// `reserving` is an allocation lock, not a published state.
/// Frozen published states: `active`, `disabled`, `retired`.
pub const SLUG_STATUS_RESERVING: &str = "reserving";
pub const SLUG_STATUS_ACTIVE: &str = "active";
pub const SLUG_STATUS_DISABLED: &str = "disabled";
pub const SLUG_STATUS_RETIRED: &str = "retired";
