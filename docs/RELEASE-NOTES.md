# BZOD v0.5.1 — Namespace Integrity & Platform Hardening

**Release Date:** 2026-06-20

BZOD v0.5.1 focuses on platform integrity, multi-tenant safety, dashboard parity, QR reliability, and upgrade validation.

While v0.5.0 introduced the multi-user architecture, v0.5.1 strengthens the foundations required for safe operation at scale.

---

# Highlights

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
