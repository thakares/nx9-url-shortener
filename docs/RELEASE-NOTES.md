# BZOD v0.6.0 — Legacy Restore Compatibility & Version Reporting

Release Date: 2026-08-09

## Highlights

- **Legacy Backup Restore Compatibility**: Backups created with the web admin "Download Backup" feature (`legacy_flat_backup` format) can now be correctly restored into the current multi-tenant database architecture. Previously, these restores failed with "no such table: users" because the restore validator ran against the empty legacy `users.db` before layout normalization.

- **CLI Version Reporting**: `bzod --version` and `bzod -V` now report the application version derived from Cargo.toml package metadata, ensuring the reported version cannot diverge from the build.

- **Deploy Script Modernization**: Removed the obsolete `init-db` command from the deployment script. Database creation and schema migration are now handled automatically by `bzod serve`. The deploy script now verifies the installed binary version using `--version`.

## Breaking Changes

None.

# BZOD v0.5.3 — Architecture Refinement & Redirect Hardening

BZOD v0.5.3 is an internal quality and maintainability release focused on architectural refinement, redirect handler hardening, and comprehensive verification.

No new user-facing features are introduced. Existing API contracts, route behavior, authentication, and tenant isolation are fully preserved.

---

# Highlights

## Modular Admin Architecture

The former monolithic admin handler file was eliminated and replaced with a focused module directory at `src/web/admin/`.

Feature modules:

* `auth.rs` — authentication and session handling
* `dashboard.rs` — dashboard rendering
* `urls.rs` — URL management handlers
* `pages.rs` — landing page management handlers
* `analytics.rs` — analytics and export handlers
* `settings.rs` — settings and configuration handlers
* `users.rs` — user management handlers
* `sessions.rs` — session administration
* `quotas.rs` — quota management
* `health.rs` — health diagnostics
* `backups.rs` — backup and restore handlers
* `api_keys.rs` — API key management
* `audit.rs` — audit log handlers
* `moderation.rs` — content moderation handlers

Benefits:

* Improved code organization and navigability
* Reduced coupling between feature areas
* Improved database lock scoping
* Reduced duplicated handler logic
* Better error handling consistency and observability
* Simplified future extension

---

## Redirect Handler Hardening

The public redirect path (`GET /:code`) was hardened against invalid HTTP Location header values.

Changes:

* Removed the panic-prone `HeaderValue::from_str(...).unwrap()` pattern
* Added destination URL validation (scheme enforcement, control character rejection)
* Added safe Location header construction that handles malformed values gracefully
* Improved database error logging with structured fields
* Reduced unnecessary database mutex lock acquisitions
* Removed synchronous expiration writes from the redirect hot path

Existing redirect security and tenant isolation behavior was preserved.

---

## Root Landing Page Verification

* Confirmed `GET /` as an intentional application route serving `www/index.html`
* Resolved a runtime path-resolution issue affecting static landing-page resolution
* Verified `GET /` returns HTTP 200
* Verified `GET /login` returns HTTP 200
* Verified `GET /admin/login` returns HTTP 200

---

# Testing & Validation

BZOD v0.5.3 passed:

* Release build (`cargo build --release`)
* Comprehensive automated test suite, including:
  * Authentication and migration tests
  * Redirect security tests
  * Root landing page test
  * Backup and restore tests
  * Business workflow tests
  * Security tests
  * Slug namespace, registry, and transfer tests
  * User management and isolation tests
  * WAL recovery tests
  * HTTP end-to-end tests
* Runtime smoke tests against the release binary
* SQLite WAL mode and foreign-key enforcement initialization
* Database migration verification (all migrations up to date)

---

# Compatibility

* No breaking changes
* No API changes
* No route changes
* No database schema changes
* No configuration changes
* Direct upgrade from v0.5.1 with no migration required

---

# Repository

* Clean source tree established
* Build artifacts, temporary reports, and IDE metadata removed
* Existing BZOD Git history preserved
* Refactoring baseline merged with existing history

---

---

# BZOD v0.5.1 — Namespace Integrity & Platform Hardening

**Release Date:** 2026-06-20

BZOD v0.5.1 focuses on platform integrity, multi-tenant safety, dashboard parity, QR reliability, and upgrade validation.

While v0.5.0 introduced the multi-user architecture, v0.5.1 strengthens the foundations required for safe operation at scale.

---

# Highlights
## Runtime Efficiency (v0.5.1)

| Metric              | Value      |
|---------------------|------------|
| Binary Size         | 11 MB      |
| RSS Memory          | 11.8 MB    |
| Peak RSS            | 11.8 MB    |
| CPU Idle            | 0.02%      |
| Swap Usage          | 0 KB       |
| PIDs                | 7          |

**On a typical 32 GB server:**
- Memory usage: ~0.04%
- No swapping
- Plenty of headroom

BZOD runs closer to a lightweight infrastructure service than a typical web application.

## Global Slug Registry

Introduced a hardened global slug registry to guarantee namespace integrity across the entire platform.

The following resources can no longer share the same slug:

* Administrator URLs
* Administrator Landing Pages
* User URLs
* User Landing Pages

Duplicate namespace conflicts are automatically detected and blocked.

---

## Namespace Integrity Validation

New validation routines now verify:

* Duplicate slug detection
* Missing ownership records
* Invalid registry entries
* Invalid target types
* Orphaned slug references

Namespace conflicts now abort upgrades and restores before corruption can occur.

---

## Reservation-Based Slug Allocation

BZOD now reserves slugs before content creation.

Creation workflow:

```text
Quota Check
↓
Reserve Global Slug
↓
Create Content
↓
Activate Slug
↓
Increment Quota
↓
Audit Log
```

Benefits:

* Prevents race conditions
* Prevents duplicate creation under concurrency
* Enables safer rollback handling

---

## Stale Reservation Recovery

Added automatic cleanup of abandoned slug reservations.

Scenarios covered:

* Server crash during creation
* Interrupted writes
* Failed transactions

BZOD now automatically recovers stale reservations during startup.

---

## Dashboard Parity

Administrator and Standard User dashboards now provide equivalent functionality where appropriate.

Added parity validation for:

* URL management
* Landing page management
* Analytics
* QR code previews
* Export functionality

Differences remain only for administrator-specific operations.

---

## Unified Analytics Templates

Removed duplicated analytics templates.

Benefits:

* Consistent rendering
* Reduced maintenance burden
* Improved reliability

Administrator and user analytics now share the same rendering logic.

---

## QR Code Improvements

QR functionality was substantially improved.

### Added

* Inline QR previews
* PNG downloads
* SVG downloads
* Shared QR rendering component

### Fixed

* Landing page QR generation
* Multi-user QR ownership handling
* QR routing consistency
* Content-type validation

---

## Canonical Landing Page Routing

Landing page slugs now redirect permanently to canonical page URLs.

Example:

```text
/landing-page
```

redirects to:

```text
/p/landing-page
```

using:

```http
301 Moved Permanently
```

This improves consistency and SEO behavior.

---

## Ownership Isolation Hardening

Additional protections ensure:

* Users cannot access another user's analytics
* Users cannot export another user's data
* Users cannot manage another user's resources

New ownership validation tests were added.

---

## Backup & Restore Improvements

Restore operations now validate namespace integrity before importing data.

Benefits:

* No silent slug collisions
* No partial restores
* No hidden ownership conflicts

Restore operations fail safely when conflicts are detected.

---

## Upgrade Validation Enhancements

Upgrade workflows now verify:

* Global namespace consistency
* Duplicate slug conflicts
* Registry integrity
* Tenant ownership correctness

Unsafe upgrades are blocked automatically.

---

## Health & Diagnostics

The system health subsystem now validates:

* Global slug registry integrity
* Namespace conflicts
* Ownership consistency
* Stale reservations

This improves operational visibility and troubleshooting.

---

# Testing & Validation

BZOD v0.5.1 passed:

* Formatting validation (`cargo fmt --check`)
* Static analysis (`cargo clippy --all-targets -- -D warnings`)
* Full automated test suite
* Namespace integrity tests
* Ownership isolation tests
* QR endpoint tests
* Dashboard parity tests
* Upgrade validation tests
* Backup & restore tests
* Disaster recovery tests
* Security tests
* Concurrency tests

All automated tests pass successfully.

---

# Upgrade Notes

Administrators upgrading from v0.5.0 should review:

* UPGRADE.md
* MULTI_USER.md
* BACKUP_RESTORE.md
* DATABASES.md
* TESTING.md

BZOD will automatically validate namespace integrity before completing upgrades.

Duplicate slugs that previously existed across users or resource types must be resolved before migration can proceed.

---

# Breaking Changes

## Global Namespace Enforcement

Slugs are now globally unique across the entire platform.

Configurations that previously relied on duplicate slugs across users or resource types will be rejected during upgrade.

This behavior is intentional and protects routing integrity.

---

# Summary

BZOD v0.5.1 is an integrity-focused release that significantly strengthens:

* Namespace safety
* Multi-tenant isolation
* Dashboard consistency
* QR reliability
* Restore safety
* Upgrade safety
* Operational diagnostics

The result is a more predictable, recoverable, and production-ready platform.
