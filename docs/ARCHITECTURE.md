# BZOD Architecture Guide

Version: v0.7.0

---

# Overview

BZOD is a self-hosted multi-user URL management platform written in Rust.

The platform combines:

* URL shortening
* Landing pages
* QR code generation
* Analytics
* User management
* Moderation
* Audit logging
* Backup & restore
* Disaster recovery

into a single deployable binary powered entirely by SQLite.

BZOD is designed around operational simplicity, tenant isolation, and long-term maintainability.

---

# Architectural Goals

The primary design goals are:

1. Self-hosted first
2. SQLite-first architecture
3. Multi-user operation
4. Tenant isolation
5. Simple deployment
6. Minimal dependencies
7. Easy backup and recovery
8. No vendor lock-in

---

# High-Level Architecture

```text
                     ┌─────────────┐
                     │   Browser   │
                     └──────┬──────┘
                            │
                            ▼
                 ┌────────────────────┐
                 │     Axum Router    │
                 └─────────┬──────────┘
                           │
      ┌────────────────────┼────────────────────┐
      │                    │                    │
      ▼                    ▼                    ▼

  users.db           system.db         User Databases

  Users              Global Slugs      content.db
  Sessions           Audit Events      analytics.db
  Quotas             Moderation
  API Tokens         Settings
```

---

# Runtime Components

## Web Layer

Location:

```text
src/web/
```

Responsible for:

* HTTP routing
* Dashboard rendering
* Form handling
* Authentication checks
* Redirect handling
* REST API endpoints

Major modules:

```text
admin/          (modular feature directory)
  auth.rs       (authentication and session handling)
  dashboard.rs  (dashboard rendering)
  urls.rs       (URL management handlers)
  pages.rs      (landing page management handlers)
  analytics.rs  (analytics and export handlers)
  settings.rs   (settings and configuration handlers)
  users.rs      (user management handlers)
  sessions.rs   (session administration)
  quotas.rs     (quota management)
  health.rs     (health diagnostics)
  backups.rs    (backup and restore handlers)
  api_keys.rs   (API key management)
  audit.rs      (audit log handlers)
  moderation.rs (content moderation handlers)
  mod.rs        (module exports and shared helpers)
api.rs
pages.rs
redirect.rs
qr.rs
system.rs
multi_user.rs
routes.rs
```

---

## Authentication Layer

Location:

```text
src/auth/
```

Responsible for:

* Password hashing
* Session validation
* Cookie management
* CSRF protection
* Authorization

Modules:

```text
csrf.rs
middleware.rs
password.rs
session.rs
```

Authentication technologies:

* Argon2id password hashing
* Session cookies
* CSRF tokens
* RBAC checks

---

## Database Layer

Location:

```text
src/db/
```

Responsible for:

* Schema creation
* Migrations
* Database access
* Analytics storage
* User management

Modules:

```text
admin.rs
analytics.rs
audit_events.rs
content.rs
migrations.rs
sqlite.rs
users.rs
```

---

# Database Architecture

BZOD uses multiple SQLite databases rather than a single monolithic database.

This approach provides:

* Better isolation
* Easier backup
* Simpler disaster recovery
* Reduced risk of cross-user data leakage

---

## users.db

Purpose:

Central identity and account database.

Contains:

```text
users
sessions
api_tokens
quotas
```

Stores:

* User accounts
* Password hashes
* Session records
* API tokens
* Quota information

---

## system.db

Purpose:

Global platform metadata.

Contains:

```text
global_slugs
audit_events
moderation_events
reserved_slugs
settings
slug_history
```

Stores:

* Global slug ownership
* Audit records
* Moderation actions
* Platform settings
* Slug transfers

---

## Tenant Databases

Each user receives isolated databases.

Directory structure:

```text
users/
└── <user_id>/
    ├── content.db
    └── analytics.db
```

---

### content.db

Stores:

* URLs
* Landing pages
* Metadata

---

### analytics.db

Stores:

* Visits
* Referrers
* QR scans
* Browser information
* Analytics aggregates

---

# Multi-User Architecture

BZOD v0.5.0 introduced complete tenant isolation.

Each user owns:

```text
content.db
analytics.db
```

Users cannot directly access:

* Other users' URLs
* Other users' landing pages
* Other users' analytics

The administrator accesses all tenants through controlled administrative interfaces.

---

# Global Slug Namespace

All public URLs are tracked in:

```text
system.db -> global_slugs
```

Purpose:

Prevent collisions across users.

Example:

```text
User A owns:

https://bzo.in/!office

User B cannot create:

https://bzo.in/!office
```

This guarantees global uniqueness.

---

# Request Lifecycle

## URL Redirect

Request:

```text
GET /abc123
```

Flow:

```text
Browser
  ↓
Axum Router
  ↓
global_slugs lookup
  ↓
Locate owner database
  ↓
Resolve URL
  ↓
Validate destination
  ↓
Record analytics
  ↓
301 Redirect (with safe Location header construction)
```

---

## Landing Page

Request:

```text
GET /p/demo
```

Flow:

```text
Browser
  ↓
Router
  ↓
global_slugs lookup
  ↓
Tenant content.db lookup
  ↓
Render page
```

---

## QR Generation

Request:

```text
GET /api/qr/demo.svg
```

Flow:

```text
Router
  ↓
global_slugs lookup
  ↓
Generate QR
  ↓
Return SVG
```

---

# Analytics Pipeline

Location:

```text
src/analytics/
```

Components:

```text
events.rs
queue.rs
worker.rs
aggregate.rs
location.rs
```

Responsibilities:

* Visit tracking
* QR tracking
* Browser detection
* Referrer parsing
* Aggregation

---

# Background Jobs

Location:

```text
src/jobs/
```

Jobs:

## aggregate.rs

Analytics aggregation.

## backup.rs

Automated backups.

## expiry.rs

Expired content cleanup.

## retention.rs

Retention policy enforcement.

## healthcheck.rs

System health validation.

## quota_reconcile.rs

Quota consistency verification.

---

# Services Layer

Location:

```text
src/services/
```

Purpose:

Business logic abstraction.

Modules:

```text
api_keys.rs
audit.rs
bulk.rs
landing_pages.rs
qr.rs
shortener.rs
```

This layer separates business rules from HTTP handlers.

---

# CLI Architecture

Location:

```text
src/cli/
```

The CLI and Web UI share the same internal services.

Examples:

```bash
bzod create-admin
bzod create-user
bzod backup
bzod restore
bzod doctor
bzod migrate
```

This avoids duplicate logic between administration methods.

---

# Security Model

Security mechanisms:

## Authentication

* Argon2id password hashes
* Session cookies

## Authorization

* RBAC
* Administrative permission checks

## CSRF Protection

* Form tokens
* Request validation

## Tenant Isolation

* Separate databases
* Controlled access paths

## Audit Logging

All critical operations are recorded.

Examples:

* Login attempts
* User creation
* Password resets
* Slug transfers
* Moderation actions

---

# Backup & Recovery

BZOD is designed for SQLite-first recovery.

Backup targets:

```text
users.db
system.db
admin/
users/*
```

Capabilities:

* Full backups
* Restore operations
* Upgrade migrations
* Disaster recovery validation

---

# Testing Architecture

Location:

```text
tests/
```

Coverage includes:

* Authentication
* Authorization
* User management
* Analytics
* Backups
* Disaster recovery
* Routing
* Security
* Concurrency
* Upgrade validation
* Multi-user isolation

The project includes comprehensive automated test coverage spanning unit, integration, security, and end-to-end tests.

---

# Deployment Models

Supported deployments:

## Native

```bash
cargo build --release
./bzod serve
```

## Systemd

```text
bzod.service
```

## Docker

```text
Dockerfile
docker-compose.yml
```

---

# Future Architecture Direction

Planned for future releases:

* Geo analytics
* OpenAPI generation
* SSO integration
* Multi-organization support
* Advanced reporting
* Distributed analytics aggregation

---

# Summary

BZOD is built around a simple principle:

> Keep deployment simple, keep data local, keep users isolated, and keep recovery easy.

The platform achieves this through:

* Rust
* Axum
* SQLite
* Tenant isolation
* Multi-database architecture
* Strong automated validation
* Operational simplicity
