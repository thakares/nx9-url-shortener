# Backup & Restore Guide

Version: v0.8.0
Applies To: BZOD Multi-User Platform

---

# Overview

BZOD provides built-in backup and recovery functionality for both single-user and multi-user deployments.

The backup architecture is designed to support:

* Full platform backups
* Individual tenant backups
* Disaster recovery
* Upgrade safety
* Migration validation
* Data integrity verification

All production deployments should maintain regular backups before performing upgrades, maintenance, or administrative operations.

---

# Database Architecture

BZOD stores data across multiple SQLite databases.

## Core Databases

```text
data/
├── users.db
├── system.db
└── users/
```

### users.db

Stores:

* User accounts
* Password hashes
* Account status
* Roles
* Sessions
* Quotas
* API tokens

### system.db

Stores:

* Global slug registry
* Reserved slugs
* Slug ownership history
* Audit events
* Moderation events
* System settings

---

## Tenant Databases

Each tenant owns isolated content and analytics databases.

```text
data/users/{user_id}/
├── content.db
└── analytics.db
```

### content.db

Stores:

* Short URLs
* Landing pages
* Metadata
* Tags
* QR code configuration

### analytics.db

Stores:

* Visit events
* Referrers
* Browser information
* Country information
* Aggregated statistics

---

# Backup Types

## Full Platform Backup

Creates a complete snapshot of the entire BZOD installation.

Includes:

```text
users.db
system.db
all tenant content.db files
all tenant analytics.db files
```

Recommended for:

* Daily scheduled backups
* Upgrades
* Server migration
* Disaster recovery

---

## User Backup

Creates a backup of a single tenant.

Includes:

```text
content.db
analytics.db
```

Recommended for:

* User export
* User migration
* User recovery

---

# CLI Backup Commands

## Create Full Backup

```bash
bzod backup
```

Output:

```text
backups/
└── backup-YYYYMMDD-HHMMSS.zip
```

---

## Create User Backup

```bash
bzod backup-user 42
```

Output:

```text
backups/
└── user-42-YYYYMMDD-HHMMSS.zip
```

---

# CLI Restore Commands

## Restore Full Backup

```bash
bzod restore backup-20260619-020000.zip
```

Restores:

* users.db
* system.db
* all tenant databases

---

## Restore Single User

```bash
bzod restore-user user-42-20260619.zip
```

Restores only:

```text
users/42/content.db
users/42/analytics.db
```

without affecting any other tenant.

---

# Web-Based Backup Management

Administrative users can manage backups through:

```text
/admin/backups
```

Features:

* Create backup
* Download backup
* Upload backup
* Restore backup
* Delete backup

Only authenticated administrators may access backup operations.

---

# Backup Strategy

## Recommended Schedule

### Daily

```text
02:00 AM
```

Create a full platform backup.

---

### Weekly

```text
Sunday 03:00 AM
```

Create a full backup and copy it to:

* NAS
* Secondary server
* External storage

---

### Monthly

Archive a backup for long-term retention.

Recommended retention:

```text
12 months
```

---

# Retention Policy

Recommended policy:

```text
Daily Backups:
30 days

Weekly Backups:
12 weeks

Monthly Backups:
12 months
```

Adjust retention according to compliance requirements.

---

# Upgrade Procedure

Always create a backup before upgrading.

## Step 1

Create backup:

```bash
bzod backup
```

## Step 2

Upgrade BZOD binary.

## Step 3

Start BZOD.

```bash
bzod serve
```

## Step 4

Allow database migrations to complete.

## Step 5

Verify:

* Login
* URLs
* Landing pages
* Analytics
* Administration panels

---

# Restore Validation

After every restore operation verify:

## Authentication

* Administrator login works
* Standard user login works

## Content

* URLs are visible
* Landing pages render correctly

## Routing

* Slug redirects work
* Landing page routes resolve

## Analytics

* Visit counts exist
* Analytics dashboards load

## System

* Audit events visible
* Moderation records preserved
* System settings preserved

## Multi-User

* Tenant isolation maintained
* Ownership mappings preserved

---

# Disaster Recovery Scenarios

## Scenario 1: Deleted User

Problem:

```text
User account accidentally deleted.
```

Recovery:

```bash
bzod restore-user user-42.zip
```

Verify:

* URLs restored
* Pages restored
* Analytics restored

---

## Scenario 2: Corrupted Tenant Database

Problem:

```text
content.db corruption
```

Recovery:

```bash
bzod restore-user user-42.zip
```

or

```bash
bzod restore full-backup.zip
```

---

## Scenario 3: Corrupted users.db

Problem:

```text
Unable to login
Missing users
Session failures
```

Recovery:

```bash
bzod restore full-backup.zip
```

---

## Scenario 4: Corrupted system.db

Problem:

```text
Slug resolution failures
Moderation data missing
Settings lost
```

Recovery:

```bash
bzod restore full-backup.zip
```

---

## Scenario 5: Complete Server Failure

Problem:

```text
Disk failure
Server loss
Hardware replacement
```

Recovery:

1. Reinstall operating system
2. Install BZOD
3. Restore backup

```bash
bzod restore backup.zip
```

4. Start BZOD

```bash
bzod serve
```

---

# WAL Mode

BZOD uses SQLite Write-Ahead Logging (WAL).

Examples:

```text
users.db
users.db-wal
users.db-shm

system.db
system.db-wal
system.db-shm

content.db
content.db-wal
content.db-shm

analytics.db
analytics.db-wal
analytics.db-shm
```

Benefits:

* Improved concurrency
* Better crash recovery
* Faster write operations

---

# Backup Safety

Do not manually copy live SQLite databases while the server is actively writing.

Always use:

```bash
bzod backup
```

or the Backup Management UI.

This ensures consistent snapshots.

---

# Security Considerations

Backups may contain:

* User accounts
* Password hashes
* Session metadata
* Analytics data
* Audit records
* API token hashes

Even though passwords and tokens are stored as hashes, backup archives should be treated as sensitive information.

Recommended practices:

* Encrypt backup storage
* Restrict filesystem permissions
* Maintain offsite copies
* Transfer backups over secure channels
* Test restores periodically

---

# Backup Testing

A backup is only useful if it can be restored.

Quarterly validation is recommended.

Example:

```bash
mkdir restore-test

bzod restore backup.zip \
    --data-dir restore-test
```

Verify:

* Login works
* URLs resolve
* Landing pages load
* Analytics display
* Administration dashboard functions

---

# Production Recommendation

Minimum production policy:

```text
Daily Full Backup
Weekly Offsite Backup
Monthly Archive Backup
Quarterly Restore Validation
```

Following this policy protects against:

* User mistakes
* Database corruption
* Upgrade failures
* Hardware failures
* Site disasters

and provides a reliable recovery path for BZOD deployments.
