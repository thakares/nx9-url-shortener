# BZOD Multi-User Architecture Guide

Version: v0.8.0

---

# Introduction

BZOD v0.5.0 introduces a complete multi-user architecture that transforms BZOD from a single-tenant URL shortener into a secure, isolated, self-hosted multi-user platform.

Each user receives logically isolated content and analytics storage while sharing a common authentication, administration, moderation, and routing infrastructure.

This document explains the architecture, database layout, ownership model, security boundaries, quotas, slug management, and administrative workflows.

---

# Design Goals

The multi-user architecture was designed around the following principles:

* Strong tenant isolation
* Single binary deployment
* SQLite-only operation
* Minimal operational complexity
* No external services required
* Global slug namespace
* Centralized administration
* Disaster recovery support
* Simple backup and restore workflows

---

# User Types

BZOD supports the following account types.

## Administrator

Administrators can:

* Access the administrative dashboard
* Create users
* Delete users
* Reset passwords
* Manage quotas
* Transfer ownership
* Moderate content
* Manage backups
* Access health dashboards
* Access audit logs

Administrators cannot bypass database isolation.

---

## Standard User

Standard users can:

* Create short URLs
* Create landing pages
* View analytics
* Generate QR codes
* Manage API tokens
* Update passwords

Standard users cannot:

* Access other user content
* Access administrative functions
* Access system settings

---

## System Accounts

System accounts are reserved for internal operations.

They cannot authenticate into the dashboard.

---

# Database Architecture

BZOD uses multiple SQLite databases.

## users.db

Central identity store.

Contains:

```text
users
sessions
quotas
api_tokens
```

Responsibilities:

* Authentication
* Session management
* Password verification
* User status management
* Quota tracking

---

## system.db

Global platform database.

Contains:

```text
global_slugs
slug_history
moderation_events
audit_events
settings
reserved_slugs
```

Responsibilities:

* Slug ownership
* Moderation
* Audit logging
* Global settings
* System metadata

---

## Tenant Databases

Every tenant owns independent databases.

Example:

```text
users/
└── 15/
    ├── content.db
    └── analytics.db
```

Responsibilities:

### content.db

Stores:

```text
urls
pages
qr_metadata
previews
```

### analytics.db

Stores:

```text
visits
aggregates
referrers
browsers
countries
```

---

# Directory Structure

Example installation:

```text
data/
├── users.db
├── system.db
│
├── admin/
│   ├── content.db
│   └── analytics.db
│
└── users/
    ├── 2/
    │   ├── content.db
    │   └── analytics.db
    │
    ├── 3/
    │   ├── content.db
    │   └── analytics.db
    │
    └── 4/
        ├── content.db
        └── analytics.db
```

---

# Global Slug Namespace

BZOD uses a platform-wide namespace.

A slug can only exist once.

Examples:

```text
/company
/about
/docs
```

If User A owns:

```text
/company
```

User B cannot create:

```text
/company
```

The operation is rejected.

---

# Slug Registration Flow

When a URL or page is created:

1. Validate quota.
2. Validate slug.
3. Register slug in system.db.
4. Create record in tenant content.db.
5. Increment quota counters.
6. Write audit log.

If any step fails:

* Changes are rolled back.
* Partial records are removed.

---

# Global Slug Table

Conceptually:

```text
global_slugs
```

Contains:

```text
slug
owner_user_id
target_type
target_id
status
created_at
```

Example:

| slug | owner | type |
| ---- | ----- | ---- |
| docs | 3     | page |
| api  | 8     | page |
| home | 2     | url  |

---

# Slug Ownership Transfer

Administrators may transfer ownership.

Process:

1. Validate destination quotas.
2. Copy content.
3. Move ownership.
4. Update global slug registry.
5. Record history.
6. Write audit event.

Analytics remain preserved.

URLs remain functional.

---

# Tenant Isolation

Each user owns independent databases.

Example:

```text
User A
└── users/2/

User B
└── users/3/
```

User A never accesses:

```text
users/3/content.db
users/3/analytics.db
```

User B never accesses:

```text
users/2/content.db
users/2/analytics.db
```

All access is enforced by application logic.

---

# Authentication Architecture

Authentication is centralized.

Stored in:

```text
users.db
```

Tables:

```text
users
sessions
```

All dashboard sessions use:

```text
bzod_session
```

Sessions are validated against:

```text
users.db.sessions
```

---

# Session Lifecycle

Login:

```text
User Login
    ↓
Create Session
    ↓
Store in users.db
    ↓
Set bzod_session cookie
```

Logout:

```text
Delete session row
    ↓
Expire cookie
```

Disabled users immediately lose access.

---

# Quota System

Every user has quotas.

Examples:

```text
max_urls
max_pages
max_storage_mb
max_api_tokens
```

Current utilization is tracked separately.

Administrators may:

* Increase limits
* Reduce limits
* Trigger reconciliation

---

# Quota Reconciliation

Background job:

```text
quota_reconcile
```

Purpose:

* Detect drift
* Recount resources
* Repair counters

Example:

```text
Stored URLs = 50
Actual URLs = 47
```

Counter automatically corrected.

---

# Analytics Isolation

Each tenant stores analytics independently.

Example:

```text
users/10/analytics.db
```

Contains only User 10 traffic.

Administrators can:

* View aggregated analytics
* Access user analytics

Users cannot view analytics from other tenants.

---

# QR Code System

QR codes are generated dynamically.

Endpoints:

```text
/api/qr/{slug}.png
/api/qr/{slug}.svg
```

Slug ownership is resolved through:

```text
system.db.global_slugs
```

No content database scan is required.

---

# Moderation Architecture

Administrators can:

* Flag content
* Disable content
* Delete content
* Transfer ownership

Disabled content returns:

```http
410 Gone
```

For:

```text
/slug
/p/slug
/api/qr/slug.png
/api/qr/slug.svg
```

---

# Audit Logging

All administrative actions are recorded.

Examples:

```text
login
logout
user_create
user_delete
password_reset
quota_update
slug_transfer
backup_create
restore_execute
```

Stored in:

```text
system.db
```

---

# Backup Architecture

Supported levels:

## Full Platform Backup

Includes:

```text
users.db
system.db
all tenant databases
```

---

## User Backup

Includes:

```text
content.db
analytics.db
```

For a specific user.

---

# Disaster Recovery

Supported operations:

```bash
bzod backup
bzod restore
bzod backup-user
bzod restore-user
```

Recovery preserves:

* URLs
* Pages
* Analytics
* Users
* Slugs
* Settings

---

# Upgrade Path

BZOD automatically migrates:

```text
v0.4.x
```

to

```text
v0.5.x
```

Migration process:

1. Create users.db.
2. Create system.db.
3. Create admin tenant.
4. Migrate content.
5. Migrate analytics.
6. Populate global_slugs.
7. Create legacy_admin.
8. Validate integrity.

No manual database migration is normally required.

---

# Security Model

Security boundaries:

## Authentication

Centralized.

```text
users.db
```

---

## Authorization

Role-based.

```text
admin
standard
system
```

---

## CSRF Protection

All forms protected.

Invalid tokens:

```http
403 Forbidden
```

---

## Session Security

* Secure session IDs
* Session invalidation
* Expiration support
* Replay protection

---

## Tenant Isolation

Per-user databases.

No shared content tables.

---

# Operational Recommendations

Recommended deployment:

```text
Nginx
    ↓
BZOD
    ↓
SQLite WAL
```

Enable:

* HTTPS
* Daily backups
* Log rotation
* Health monitoring

---

# Limitations

Current v0.5.0 limitations:

* SQLite backend only
* Single server deployment
* No clustering
* No federation
* No organization account hierarchy

These may be addressed in future releases.

---

# Future Expansion

Potential v0.6.x features:

* Organization accounts
* Service accounts
* SSO integration
* Multi-node replication
* Advanced analytics dashboards
* Scheduled tasks UI

---

# Summary

BZOD v0.5.0 provides:

* Centralized authentication
* Multi-user isolation
* Global slug namespace
* Per-user analytics
* Administrative moderation
* Quotas
* Audit logging
* Backup & disaster recovery
* Single-binary deployment

while remaining lightweight, SQLite-native, and operationally simple.

---

End of Document.
