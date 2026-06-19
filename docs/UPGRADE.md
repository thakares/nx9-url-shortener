# Upgrade Guide

Version: v0.5.0

This document describes the upgrade process from previous BZOD releases to BZOD v0.5.0.

---

# Overview

BZOD v0.5.0 introduces the largest architectural change in project history:

* Multi-user architecture
* Tenant isolation
* Global slug namespace
* Centralized authentication
* User quotas
* User-specific analytics
* Administrative user management
* Backup and restore framework

Existing v0.4.x deployments can be upgraded without data loss.

---

# Supported Upgrade Paths

Supported:

```text
v0.4.0  → v0.5.0
v0.4.x  → v0.5.0
```

Unsupported:

```text
v0.3.x → v0.5.0
```

Older installations should first upgrade to v0.4.x.

---

# Breaking Changes

## Database Layout

### v0.4.x

```text
data/
├── admin.db
├── content.db
└── analytics.db
```

### v0.5.0

```text
data/
├── users.db
├── system.db
└── users/
    └── 1/
        ├── content.db
        └── analytics.db
```

---

## Authentication

Authentication is now centralized.

Old:

```text
admin.db
```

New:

```text
users.db
```

Sessions are managed globally.

---

## Global Slug Namespace

Slugs are now unique platform-wide.

Examples:

```text
/example
/company
/docs
```

cannot exist twice.

---

# Pre-Upgrade Checklist

Before upgrading:

* Verify current version
* Stop active traffic
* Create backup
* Verify backup integrity

---

## Step 1: Create Backup

CLI:

```bash
bzod backup
```

or manually archive:

```bash
tar czf bzod-backup.tar.gz data/
```

---

## Step 2: Verify Backup

Confirm archive contains:

```text
admin.db
content.db
analytics.db
```

---

## Step 3: Stop Service

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

## Replace Binary

Install new release:

```bash
cargo build --release
```

or download release binary.

---

## Start BZOD

```bash
bzod serve
```

On first startup BZOD automatically:

1. Detects legacy databases.
2. Creates users.db.
3. Creates system.db.
4. Creates administrator tenant.
5. Moves content.db.
6. Moves analytics.db.
7. Creates global slug registry.
8. Runs migrations.

---

# Automatic Migration

Migration performs:

## Administrator Creation

Legacy administrator becomes:

```text
User ID: 1
Type: admin
```

---

## Content Migration

All URLs migrate into:

```text
users/1/content.db
```

---

## Analytics Migration

All analytics migrate into:

```text
users/1/analytics.db
```

---

## Global Slug Registration

All existing slugs are inserted into:

```text
system.db.global_slugs
```

---

# Post-Upgrade Validation

## Login

Verify:

```text
Admin login succeeds
```

---

## URLs

Verify:

```text
Short URLs redirect
```

Example:

```text
https://example.com/abc123
```

---

## Landing Pages

Verify:

```text
https://example.com/p/demo
```

renders correctly.

---

## Analytics

Verify:

* Visits visible
* Reports load
* Charts render

---

## User Management

Verify:

```text
Admin → Users
```

loads correctly.

---

# Upgrade Validation Tests

BZOD v0.5.0 includes automated migration tests.

Validated:

* Legacy admin migration
* Legacy content migration
* Legacy analytics migration
* Slug registration
* Redirect preservation
* Analytics preservation

Test suite:

```bash
cargo test --test upgrade_validation_tests
```

---

# Rollback Procedure

If upgrade validation fails:

## Stop Server

```bash
sudo systemctl stop bzod
```

or

```bash
docker compose down
```

---

## Restore Backup

```bash
bzod restore backup.zip
```

or restore archived data directory.

---

## Reinstall Previous Release

Deploy previous v0.4.x binary.

---

# Docker Upgrade

Pull new image:

```bash
docker compose pull
```

Restart:

```bash
docker compose up -d
```

Monitor logs:

```bash
docker compose logs -f
```

Verify migrations complete successfully.

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

---

# Recommended Upgrade Workflow

```text
1. Create backup
2. Stop service
3. Install v0.5.0
4. Start service
5. Run migrations
6. Validate login
7. Validate URLs
8. Validate analytics
9. Validate admin dashboard
10. Return to production
```

---

# Troubleshooting

## Login Fails

Check:

```text
users.db
```

Verify administrator account exists.

---

## URLs Missing

Verify:

```text
users/1/content.db
```

contains migrated records.

---

## Analytics Missing

Verify:

```text
users/1/analytics.db
```

contains visit data.

---

## Slug Resolution Fails

Verify:

```sql
SELECT * FROM global_slugs;
```

returns expected entries.

---

# Upgrade Status

BZOD v0.5.0 upgrade path has been validated through automated migration and integration testing and is considered production-ready for upgrades from v0.4.x deployments.
