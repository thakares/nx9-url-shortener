# DATABASES.md

# BZOD Database Architecture

BZOD v0.5.0 uses SQLite exclusively.

Rather than using a single monolithic database, BZOD separates data into administrative and tenant-specific databases. This architecture improves security, isolation, backup flexibility, disaster recovery, and scalability.

---

# Overview

BZOD stores data in the following structure:

```text
data/
├── admin/
│   ├── admin.db
│   ├── system.db
│   └── users.db
│
└── users/
    ├── 1/
    │   ├── analytics.db
    │   ├── content.db
    │   └── profile.db
    │
    ├── 2/
    │   ├── analytics.db
    │   ├── content.db
    │   └── profile.db
    │
    └── N/
        ├── analytics.db
        ├── content.db
        └── profile.db
```

Each user receives isolated databases.

No user content or analytics are stored in the central administrative databases.

---

# Administrative Databases

Administrative databases are located under:

```text
data/admin/
```

---

# users.db

Primary authentication and user management database.

Purpose:

* User accounts
* Password hashes
* Sessions
* Quotas
* API tokens
* User status tracking

Typical tables:

```text
users
sessions
quotas
api_tokens
```

Responsibilities:

* Authentication
* Authorization
* Session management
* Account status
* Quota enforcement

This is the primary identity database of the platform.

---

# system.db

Global platform database.

Purpose:

* Global slug namespace
* Moderation
* Auditing
* System configuration

Typical tables:

```text
global_slugs
slug_history
moderation_events
audit_events
reserved_slugs
settings
```

Responsibilities:

* Global slug uniqueness
* Slug ownership
* Moderation actions
* Audit logging
* System settings

Every redirect ultimately resolves through records stored in this database.

---

# admin.db

Administrative application database.

Purpose:

* Administrative metadata
* Administrative API key records
* Legacy compatibility structures
* Internal management data

Typical tables:

```text
api_keys
audit_events
```

This database is reserved for administrative functions and does not store tenant content.

---

# Tenant Databases

Tenant databases are located under:

```text
data/users/{user_id}/
```

Each user owns a completely isolated set of databases.

Example:

```text
data/users/2/
├── analytics.db
├── content.db
└── profile.db
```

---

# content.db

Stores user-owned content.

Purpose:

* Short URLs
* Landing pages
* QR metadata
* Preview metadata

Typical tables:

```text
urls
pages
qr_codes
previews
```

Responsibilities:

* URL management
* Landing page management
* Content ownership

This database contains the actual resources owned by a user.

---

# analytics.db

Stores traffic and visitor information.

Purpose:

* Visit recording
* Referrer tracking
* Browser tracking
* Country statistics
* Aggregated analytics

Typical tables:

```text
visits
referrers
browsers
countries
daily_stats
```

Responsibilities:

* Analytics collection
* Reporting
* Dashboard statistics

Analytics are fully isolated per user.

Administrators access aggregated analytics by querying each user's analytics database.

---

# profile.db

Stores user-specific profile information.

Purpose:

* User preferences
* Profile settings
* Future extensible metadata

Typical tables:

```text
profile
preferences
```

Responsibilities:

* User profile management
* Dashboard preferences
* Future personalization features

---

# Database Isolation Model

BZOD follows a strict tenant isolation model.

```text
User A
 ├── content.db
 ├── analytics.db
 └── profile.db

User B
 ├── content.db
 ├── analytics.db
 └── profile.db
```

User databases never share tables.

Cross-user content access is prevented by design.

Benefits:

* Security
* Easier backups
* Easier deletion
* Reduced corruption impact

---

# Global Slug Registry

The system maintains a single namespace.

Stored in:

```text
system.db
```

Table:

```text
global_slugs
```

Example:

```text
abc123 → User 2 URL
docs → User 5 Page
demo → User 1 URL
```

This guarantees:

* Global uniqueness
* Ownership tracking
* Moderation support
* Slug transfer support

---

# Write Flow

Creating a URL:

```text
1. Validate quota
2. Register slug in system.db
3. Create URL in content.db
4. Update quota counters
5. Write audit event
```

Creating a landing page:

```text
1. Validate quota
2. Register slug in system.db
3. Create page in content.db
4. Update quota counters
5. Write audit event
```

---

# Analytics Flow

Visitor request:

```text
GET /abc123
```

Process:

```text
global_slugs
        ↓
content.db lookup
        ↓
redirect
        ↓
analytics.db visit record
```

Analytics writes never modify content records.

---

# WAL Mode

All databases operate in SQLite WAL mode.

Verify:

```sql
PRAGMA journal_mode;
```

Expected:

```text
wal
```

Benefits:

* Improved concurrency
* Reduced write contention
* Crash recovery

Associated files:

```text
*.db
*.db-shm
*.db-wal
```

---

# WAL Checkpointing

Large WAL files are normal during heavy traffic.

Example:

```text
analytics.db-wal
content.db-wal
```

To manually checkpoint:

```sql
PRAGMA wal_checkpoint(TRUNCATE);
```

The healthcheck and backup jobs may trigger checkpoints automatically.

---

# Backups

Recommended:

```bash
bzod backup
```

This creates a consistent archive of:

```text
admin/
users/
```

Never manually copy live databases while the application is running.

---

# Integrity Verification

Run:

```bash
bzod doctor
```

Or:

```sql
PRAGMA integrity_check;
```

Expected:

```text
ok
```

---

# Migration System

BZOD maintains schema versions using:

```sql
PRAGMA user_version;
```

Startup automatically executes:

```text
Db::init()
```

which:

1. Creates missing databases
2. Applies migrations
3. Validates schemas
4. Repairs legacy installations when required

---

# Design Principles

BZOD database architecture prioritizes:

* SQLite-only deployment
* Multi-user isolation
* Operational simplicity
* Backup friendliness
* Easy disaster recovery
* Minimal dependencies
* Single-binary deployment

---

# Related Documentation

* ARCHITECTURE.md
* MULTI_USER.md
* BACKUP_RESTORE.md
* INSTALL.md
* UPGRADE.md
* SECURITY.md
