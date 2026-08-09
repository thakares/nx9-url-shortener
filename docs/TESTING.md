# BZOD Testing & Validation Guide

## Overview

BZOD follows a defense-in-depth validation strategy.

A release is considered valid only when:

* Code quality checks pass
* Automated tests pass
* Upgrade validation passes
* Backup/restore validation passes
* Namespace integrity validation passes
* Multi-user isolation validation passes
* Disaster recovery validation passes

The objective is not simply to ensure the application starts, but to ensure that it can be safely upgraded, operated, backed up, restored, and recovered.

---

# Validation Philosophy

BZOD prioritizes:

1. Namespace Integrity
2. Data Integrity
3. Multi-Tenant Isolation
4. Operational Simplicity
5. Recovery Capability
6. Security
7. Functional Correctness

A successful release is not merely one that runs.

A successful release is one that can be recovered.

---

# Automated Test Coverage

Current validation suite includes:

* Unit Tests
* Integration Tests
* HTTP E2E Tests
* Business Workflow Tests
* Security Tests
* Backup & Restore Tests
* Disaster Recovery Tests
* Migration Tests
* Upgrade Validation Tests
* Namespace Integrity Tests
* Ownership Isolation Tests
* Dashboard Parity Tests
* QR Endpoint Tests
* Concurrency Tests
* WAL Recovery Tests

The platform currently executes approximately 100+ automated tests.

---

# 1. Build Validation

Verify successful compilation.

```bash
cargo check
cargo build
cargo build --release
```

Expected:

* No compilation failures
* Release binary generated

---

# 2. Formatting Validation

```bash
cargo fmt --check
```

Expected:

* No formatting errors

---

# 3. Static Analysis

```bash
cargo clippy --all-targets -- -D warnings
```

Expected:

* Zero warnings
* Zero errors

---

# 4. Complete Automated Test Suite

```bash
cargo test --all-targets -- --nocapture
```

Expected:

* All tests pass
* No failures
* No ignored critical tests

---

# 5. Database Initialization Validation

Create clean environment:

```bash
rm -rf data
```

Run:

```bash
bzod stats
```

Expected:

* Database hierarchy created
* Migrations applied
* System healthy

Validate:

```bash
bzod doctor
```

Expected:

```text
Overall status: HEALTHY
```

---

# 6. Namespace Integrity Validation

BZOD maintains a global slug namespace.

The following must never coexist:

```text
Admin URL
hello

User URL
hello

Landing Page
hello
```

Validate:

```bash
bzod doctor
```

Expected:

```text
No namespace conflicts detected
```

Duplicate slugs must abort upgrade and restore operations.

---

# 7. Multi-User Isolation Validation

Verify:

* User A cannot access User B URLs
* User A cannot access User B Pages
* User A cannot access User B Analytics
* User A cannot export User B analytics

Expected:

```http
403 Forbidden
```

for all unauthorized access.

---

# 8. Dashboard Parity Validation

Verify:

## Administrator URLs

Contains:

* Analytics
* QR Preview
* PNG Download
* SVG Download

## User URLs

Contains identical functionality.

Differences allowed:

* User Management
* Moderation
* Backups
* Health
* Audit
* Quotas

Everything else must match.

---

# 9. Analytics Validation

Verify:

* URL Analytics
* Landing Page Analytics
* CSV Export
* JSON Export
* Date Filters
* Charts
* Referrer Breakdown
* Country Breakdown
* Browser Breakdown
* Device Breakdown

Expected:

Administrator and owner views return identical analytics.

---

# 10. QR Validation

Verify:

```text
/api/qr/<slug>.png
/api/qr/<slug>.svg
```

Expected:

```http
200 OK
```

Verify:

```text
Content-Type: image/png
Content-Type: image/svg+xml
```

Disabled resources:

```http
410 Gone
```

Missing resources:

```http
404 Not Found
```

---

# 11. Routing Validation

URL resources:

```text
/<slug>
```

must redirect correctly.

Landing Pages:

```text
/<slug>
```

must redirect permanently to:

```text
/p/<slug>
```

Expected:

```http
301 Moved Permanently
```

and:

```http
200 OK
```

for final landing page render.

Root landing page:

```text
GET /
```

must serve the static landing page.

Expected:

```http
200 OK
Content-Type: text/html
```

Redirect security:

Redirect destinations are validated against:

* Invalid URL schemes
* CRLF injection attempts
* Control character injection
* Malformed HTTP Location header values

Invalid destinations must return:

```http
500 Internal Server Error
```

and must not panic or produce malformed HTTP responses.

---

# 12. Backup Validation

Create backup:

```bash
bzod backup
```

Expected:

Archive generated successfully.

Validate archive contents.

---

# 13. Restore Validation

Restore backup:

```bash
bzod restore --file backup.tar.gz
```

Expected:

* Restore succeeds
* All data preserved
* Namespace integrity preserved

---

# 14. Collision Protection Validation

Attempt restore containing duplicate slugs.

Expected:

```text
Restore aborted
Slug conflict detected
```

No partial restore.

---

# 15. Upgrade Validation

Verify upgrade from legacy deployments.

Expected:

* User databases migrated
* Analytics preserved
* Links preserved
* Landing pages preserved
* Authentication preserved

Duplicate slugs must abort upgrade.

---

# 16. Disaster Recovery Validation

Procedure:

1. Backup system
2. Stop service
3. Remove data directory
4. Restore backup
5. Start service

Expected:

* Full recovery
* No manual repair
* All URLs functional
* All Landing Pages functional
* Analytics preserved

---

# 17. Docker Validation

```bash
docker compose build --no-cache
docker compose up -d
```

Verify:

```bash
docker compose logs
```

Expected:

```text
Server started successfully
```

Container health:

```text
healthy
```

---

# 18. WAL Recovery Validation

Verify:

* SQLite WAL mode enabled
* Recovery after backup succeeds
* No corruption detected

---

# Release Validation Checklist

Before every release:

```bash
cargo fmt --check

cargo clippy --all-targets -- -D warnings

cargo test --all-targets -- --nocapture

cargo build --release

cargo audit
```

Release is approved only if all steps succeed.

---

# Release Blockers

The following are release blockers:

* Namespace conflicts
* Backup failure
* Restore failure
* Upgrade failure
* Multi-user isolation failure
* Ownership validation failure
* Security test failure
* Data corruption
* Disaster recovery failure

A release that cannot be restored is not considered production ready.
