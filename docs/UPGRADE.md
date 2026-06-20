# Upgrade Guide

Version: v0.5.1

This document describes the upgrade process for existing BZOD deployments upgrading to BZOD v0.5.1.

---

# Overview

BZOD v0.5.1 is a platform hardening release focused on:

* Global namespace integrity
* Multi-tenant safety
* Dashboard parity
* QR reliability
* Upgrade validation
* Restore collision protection
* Ownership isolation

While v0.5.0 introduced the multi-user architecture, v0.5.1 strengthens the operational and data integrity guarantees required for production deployments.

---

# Supported Upgrade Paths

Supported:

```text
v0.5.0 → v0.5.1
v0.4.x → v0.5.1
```

Recommended:

```text
v0.4.x → v0.5.0 → v0.5.1
```

Unsupported:

```text
v0.3.x → v0.5.1
```

Older installations should first upgrade to v0.4.x.

---

# Major Changes in v0.5.1

## Global Namespace Enforcement

BZOD now enforces a single platform-wide slug namespace.

The following resources can no longer share the same slug:

* Administrator URLs
* Administrator Landing Pages
* User URLs
* User Landing Pages

Example:

```text
Admin URL:
hello

User URL:
hello
```

Result:

```text
Upgrade aborted.
Namespace conflict detected.
```

---

## Global Slug Registry

BZOD now treats the slug registry as the authoritative source of truth.

All slugs are registered in:

```text
system.db
```

Table:

```text
global_slugs
```

The registry tracks:

```text
slug
owner_user_id
target_type
target_id
status
```

---

## Reservation-Based Slug Allocation

Slug creation now follows:

```text
Quota Validation
↓
Reserve Global Slug
↓
Create Resource
↓
Activate Slug
↓
Update Quotas
↓
Audit Log
```

Benefits:

* Prevents race conditions
* Prevents duplicate allocations
* Improves rollback safety
* Improves multi-user integrity

---

## Stale Reservation Recovery

BZOD automatically cleans abandoned reservations created by:

* Server crashes
* Interrupted requests
* Failed transactions

Stale reservations are validated and cleaned during startup.

---

# Breaking Changes

## Global Slug Uniqueness

Deployments containing duplicate slugs will not upgrade.

Example:

```text
User 1:
!nx9-dns-server

User 3:
!nx9-dns-server
```

Result:

```text
Upgrade aborted.

Database upgrade aborted due to slug conflicts.
```

Conflicts must be resolved before migration can continue.

---

## Restore Collision Protection

Restore operations now validate namespace integrity.

Example:

```text
Existing slug:
company

Backup slug:
company
```

Result:

```text
Restore aborted.
Slug conflict detected.
```

No partial restore occurs.

---

# Pre-Upgrade Checklist

Before upgrading:

* Create backup
* Verify backup integrity
* Stop active traffic
* Run diagnostics
* Resolve namespace conflicts

---

# Step 1: Create Backup

Full backup:

```bash
bzod backup
```

Manual backup:

```bash
tar czf bzod-backup.tar.gz data/
```

---

# Step 2: Verify Backup

Verify archive contents:

```text
users.db
system.db

users/
```

If upgrading from legacy versions:

```text
admin.db
content.db
analytics.db
```

should also be present.

---

# Step 3: Run Diagnostics

Execute:

```bash
bzod doctor
```

Expected:

```text
Overall Status: HEALTHY
```

Verify:

```text
No namespace conflicts detected
No ownership violations detected
No registry corruption detected
```

---

# Step 4: Stop Service

Systemd:

```bash
sudo systemctl stop bzod
```

Docker:

```bash
docker compose down
```

---

# Upgrade Procedure

## Install New Version

Build:

```bash
cargo build --release
```

Or install official release binary.

---

## Start BZOD

```bash
bzod serve
```

or:

```bash
docker compose up -d
```

---

# Automatic Upgrade Actions

During startup BZOD automatically performs:

1. Database migration checks
2. Namespace integrity validation
3. Registry validation
4. Stale reservation cleanup
5. Global slug verification
6. Schema migration execution

---

# Namespace Validation

BZOD scans:

```text
legacy databases
administrator databases
tenant databases
```

for duplicate slugs.

Example:

```text
Owner 1:
hello

Owner 3:
hello
```

Result:

```text
Namespace conflict detected.
Upgrade aborted.
```

---

# Registry Validation

BZOD validates:

* Duplicate slug entries
* Missing owners
* Missing targets
* Invalid target types
* Invalid status values

Allowed target types:

```text
url
page
```

Allowed statuses:

```text
reserving
active
disabled
```

---

# Post-Upgrade Validation

Run:

```bash
bzod doctor
```

Expected:

```text
Namespace Integrity: PASS
Registry Integrity: PASS
Ownership Integrity: PASS
Database Integrity: PASS
```

---

# Login Validation

Verify:

```text
Administrator login succeeds
User login succeeds
```

---

# URL Validation

Verify:

```text
https://example.com/abc123
```

redirects correctly.

Expected:

```http
302 Found
```

or configured redirect behavior.

---

# Landing Page Validation

Verify:

```text
https://example.com/p/demo
```

renders successfully.

Verify:

```text
https://example.com/demo
```

redirects permanently:

```http
301 Moved Permanently
```

to:

```text
/p/demo
```

---

# QR Validation

Verify:

```text
/api/qr/demo.png
/api/qr/demo.svg
```

Expected:

```http
200 OK
```

Content types:

```text
image/png
image/svg+xml
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

# Dashboard Validation

Verify Administrator Dashboards:

* URLs
* Landing Pages
* Analytics
* QR Preview
* PNG Download
* SVG Download

Verify Standard User Dashboards:

* URLs
* Landing Pages
* Analytics
* QR Preview
* PNG Download
* SVG Download

Both should provide equivalent functionality except for administrator-only operations.

---

# Ownership Isolation Validation

Verify:

```text
User A
```

cannot access:

```text
User B Analytics
User B URLs
User B Landing Pages
User B Exports
```

Expected:

```http
403 Forbidden
```

---

# Backup & Restore Validation

Create backup:

```bash
bzod backup
```

Restore backup:

```bash
bzod restore backup.tar.gz
```

Expected:

* No namespace conflicts
* No ownership conflicts
* No partial restores

---

# Rollback Procedure

If upgrade validation fails:

Stop service:

```bash
sudo systemctl stop bzod
```

or:

```bash
docker compose down
```

Restore backup:

```bash
bzod restore backup.tar.gz
```

or restore archived data directory.

Reinstall previous release.

---

# Docker Upgrade

Pull image:

```bash
docker compose pull
```

Restart:

```bash
docker compose up -d
```

Monitor:

```bash
docker compose logs -f
```

Expected:

```text
Namespace validation passed
Registry validation passed
Server started successfully
```

---

# Systemd Upgrade

Replace binary:

```bash
sudo cp bzod /usr/local/bin/
```

Restart:

```bash
sudo systemctl restart bzod
```

Verify:

```bash
sudo systemctl status bzod
```

Expected:

```text
active (running)
```

---

# Automated Upgrade Validation

Execute:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets -- --nocapture
```

Particularly validate:

```text
upgrade_validation_tests
backup_restore_tests
slug_registry_tests
ownership_tests
analytics_parity_tests
transaction_tests
```

---

# Recommended Upgrade Workflow

```text
1. Create Backup
2. Verify Backup
3. Run bzod doctor
4. Resolve Namespace Conflicts
5. Stop Service
6. Install v0.5.1
7. Start Service
8. Validate Registry
9. Validate URLs
10. Validate Landing Pages
11. Validate QR Endpoints
12. Validate Dashboards
13. Validate Ownership Isolation
14. Return To Production
```

---

# Troubleshooting

## Upgrade Aborted Due To Slug Conflicts

Example:

```text
Slug '!nx9-dns-server'
is defined in multiple content databases
by owners [1,3]
```

Cause:

```text
Duplicate slug detected.
```

Resolution:

```text
Rename or remove conflicting resources.
Restart upgrade.
```

---

## QR Codes Return 404

Verify:

```text
global_slugs
```

contains the slug.

Verify slug status:

```text
active
```

---

## Landing Page Redirect Fails

Verify:

```text
target_type = page
```

in:

```text
global_slugs
```

---

## Ownership Errors

Run:

```bash
bzod doctor
```

Verify ownership integrity passes.

---

# Upgrade Status

BZOD v0.5.1 upgrade path has been validated through:

* Migration Tests
* Upgrade Validation Tests
* Namespace Integrity Tests
* Ownership Isolation Tests
* Backup & Restore Tests
* Dashboard Parity Tests
* QR Endpoint Tests
* Routing Tests

The v0.5.1 upgrade path is considered production-ready.
