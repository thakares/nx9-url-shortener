# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and this project follows Semantic Versioning.

---

# v0.7.0 — Responsive UI, Theme Support & Build Metadata

## Added

### Responsive UI

* Responsive layouts for admin and user URL registry panels
* Responsive layouts for admin and user landing page registry panels
* Desktop, laptop, tablet, and mobile layout support
* Table-to-card responsive behavior for registry panels
* Resolved horizontal scrolling issues in registry panels

### Theme Support

* Dark/light theme toggle
* Theme persistence across sessions
* Responsive theme behavior across device sizes

### Build Metadata

* Introduced `build.rs` build script for compile-time metadata
* Introduced `src/build_info.rs` module exposing `APP_VERSION` and `GIT_COMMIT`
* Application version derived from `Cargo.toml` via `env!("CARGO_PKG_VERSION")`
* Git commit hash (12-char short) embedded at build time via `BZOD_GIT_COMMIT`
* Graceful fallback to `"unknown"` when Git metadata is unavailable

### Public Landing Page

* Root `/` serves `www/index.html` with runtime file and embedded fallback behavior
* Public/runtime www assets supported by the deployment layout

## Changed

* Documentation updated to reflect v0.7.0 current state
* Version metadata updated across Cargo.toml, deploy.sh, and docker-compose.yml

## Notes

* No API behavior changes
* No database schema changes
* No authentication or security behavior changes
* Existing redirect, routing, and tenant isolation behavior preserved

---

# v0.6.0 — Legacy Restore Compatibility & Version Reporting

- **Legacy Backup Restore**: Full backward-compatible restore support for `legacy_flat_backup` archives into the current multi-tenant database architecture
- **CLI Version Reporting**: Added `--version` / `-V` flags derived from Cargo package metadata
- **Deploy Script**: Removed obsolete `init-db` command; database creation and migration now handled by `bzod serve`
- **Version Verification**: Deploy script now verifies installed binary version matches requested version

# v0.5.3 — Architecture Refinement & Redirect Hardening

---

## Changed

### Architecture

* Eliminated the monolithic `admin.rs` handler file
* Reorganized admin functionality into focused feature modules under `src/web/admin/`
* Separated authentication, dashboard, URLs, pages, analytics, settings, users, sessions, quotas, health, backups, API keys, audit, and moderation into dedicated modules
* Extracted shared authentication and authorization helpers
* Extracted common export and helper functionality

### Redirect Handling

* Removed panic-prone `HeaderValue::from_str(...).unwrap()` pattern from the redirect path
* Added destination URL validation (scheme validation, control character rejection)
* Added safe HTTP Location header construction
* Improved database error logging with structured fields
* Reduced unnecessary database mutex lock acquisitions on the redirect hot path
* Removed synchronous expiration writes from the redirect hot path

---

## Improved

* Database lock scoping across admin handlers
* Error handling consistency and observability
* Handler decomposition for oversized functions
* Reduced duplicated handler logic across admin operations

---

## Verified

* Root landing page (GET /) confirmed as intentional route serving www/index.html
* Release binary built successfully
* Runtime smoke tests passed (GET /, GET /login, GET /admin/login all return HTTP 200)
* SQLite WAL mode and foreign-key enforcement initialized successfully
* All existing migrations reported as up to date
* Comprehensive automated test suite passed, including:
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

---

## Notes

* This release is an internal architecture and quality improvement
* No new user-facing features were introduced
* Existing API and route behavior was preserved
* Existing redirect security and tenant isolation behavior was preserved

---

# v0.5.1 - General Availability (GA)

Release Date: 2026-06-20

BZOD v0.5.1 is the largest release since project inception, transforming BZOD from a single-user URL shortener into a complete multi-user redirector, landing page, analytics, and administration platform.

---

## Added

### Multi-User Platform

* Multi-user architecture with isolated tenant databases
* Standard user accounts
* Administrator accounts
* User provisioning and lifecycle management
* User enable/disable operations
* User deletion workflows
* Password reset functionality
* User quota management
* User database isolation

### Authentication & Security

* Session-based authentication
* CSRF protection
* Role-Based Access Control (RBAC)
* Password hashing and verification
* Session invalidation
* Login/logout workflows
* Administrative privilege separation
* Audit logging

### User Self-Service Portal

* User dashboard
* My Links management
* My Pages management
* User analytics dashboard
* API token management
* Password management
* Profile management

### Administration

* User management dashboard
* User detail pages
* User creation forms
* User editing interface
* Session administration
* Quota administration
* Moderation dashboard
* Slug management dashboard
* Audit event viewer
* Backup management interface
* System health dashboard

### Analytics

* Per-user analytics
* URL analytics dashboards
* Landing page analytics dashboards
* Browser statistics
* Referrer tracking
* Visit logging
* Geographic analytics framework
* Analytics aggregation jobs

### Content Management

* Landing page builder
* URL registry management
* Global slug namespace
* Slug ownership tracking
* Slug transfer workflows
* Soft delete support
* Moderation controls

### Operations

* Backup CLI
* Restore CLI
* User backup support
* User restore support
* Database diagnostics
* Health checks
* Quota reconciliation jobs
* Retention jobs
* Expiry jobs
* Aggregation workers

### Documentation

* Installation Guide
* Upgrade Guide
* Multi-User Guide
* Administration Guide
* Security Guide
* Backup & Restore Guide
* Database Documentation
* Architecture Documentation
* CLI Documentation
* API Documentation
* Testing Documentation

---

## Changed

### Architecture

* Migrated from single-user storage model to tenant-isolated storage model
* Introduced users.db as central identity store
* Introduced system.db as global platform metadata store
* Introduced per-user content databases
* Introduced per-user analytics databases

### Routing

* Unified global slug resolution
* Centralized slug ownership tracking
* Improved redirect handling
* Improved landing page routing

### Analytics

* Improved aggregation performance
* Improved reporting consistency
* Improved analytics isolation

### Administration

* Expanded administrative tooling
* Improved dashboard coverage
* Added operational visibility

---

## Security

### Added

* CSRF validation
* Session management
* RBAC enforcement
* Audit event logging
* User isolation controls
* Slug ownership validation

### Hardened

* Authentication flows
* Session validation
* Administrative authorization
* User lifecycle operations

---

## Database

### Added

* users.db
* system.db
* Per-user content.db
* Per-user analytics.db
* Migration framework

### Improved

* WAL mode support
* Upgrade migrations
* Backup compatibility
* Recovery workflows

---

## Testing

### Added

Comprehensive automated validation covering:

* Authentication tests
* Authorization tests
* Migration tests
* Upgrade validation tests
* User isolation tests
* Slug namespace tests
* Slug transfer tests
* Moderation tests
* Backup and restore tests
* Disaster recovery tests
* Analytics tests
* Concurrency tests
* HTTP end-to-end tests
* Business workflow tests
* Security regression tests

### Coverage

* 90+ unit and integration tests
* HTTP workflow validation
* Upgrade path verification
* Multi-user isolation verification
* Backup and recovery validation

---

## Fixed

### Authentication

* Multi-user migration login regressions
* Session validation issues
* Administrative account migration edge cases

### Routing

* Redirect handling consistency
* Slug ownership synchronization
* Landing page resolution issues

### Analytics

* Aggregation edge cases
* Reporting consistency
* Isolation validation

### Concurrency

* Fixed mutex deadlock conditions discovered during E2E testing
* Improved lock scoping around audit logging

### Administration

* Improved slug transfer workflows
* Improved user lifecycle operations
* Improved dashboard consistency

---

## Upgrade Notes

### From v0.4.0

BZOD v0.5.0 introduces a new multi-user architecture.

Existing installations are automatically migrated during startup.

Migration includes:

* Legacy administrator migration
* Global slug index generation
* User database creation
* Analytics preservation
* Content preservation

Backups are strongly recommended before upgrading.

---

# v0.4.0

## Added

* Raw visitor activity logs
* Analytics drill-down pages
* Date-range analytics filters
* CSV export
* JSON export
* Advanced pagination
* Visitor log tables

## Improved

* Registry pagination
* Analytics navigation
* Export performance

## Fixed

* Pagination edge cases
* Analytics sorting consistency
