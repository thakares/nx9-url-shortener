# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and this project follows Semantic Versioning.

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
