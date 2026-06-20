# BZOD

> Self-hosted Multi-User URL Management, Landing Page and QR Analytics Platform written in Rust.

![Rust](https://img.shields.io/badge/Rust-Stable-orange)
![SQLite](https://img.shields.io/badge/SQLite-Embedded-blue)
![License](https://img.shields.io/badge/License-MIT%20%2F%20Apache--2.0-green)
![Version](https://img.shields.io/badge/Version-v0.5.1-purple)

BZOD combines URL shortening, landing pages, QR code generation, analytics, moderation, audit logging, backup & restore workflows, tenant isolation, and administrative tooling into a single deployable binary powered entirely by SQLite.

Designed for:

* Homelabs
* Small Businesses
* Enterprises
* Educational Institutions
* Government Agencies
* Internal IT Platforms
* Self-Hosted Enthusiasts

BZOD enables complete ownership of:

* Links
* Analytics
* Users
* QR Codes
* Landing Pages
* Operational Data

without requiring PostgreSQL, Redis, Elasticsearch, Kubernetes, or external SaaS services.

---

# Why BZOD?

Most URL shorteners focus only on redirects and click tracking.

BZOD is designed as a complete self-hosted platform that combines:

* URL Management
* Landing Pages
* QR Codes
* Analytics
* User Management
* Audit Logging
* Moderation
* Backup & Restore
* Disaster Recovery
* Administrative Tooling

within a single deployable application.

The goal is operational simplicity without sacrificing reliability, security, or ownership.

---
## Runtime Efficiency (v0.5.1)

| Metric              | Value      |
|---------------------|------------|
| Binary Size         | 11 MB      |
| RSS Memory          | 11.8 MB    |
| Peak RSS            | 11.8 MB    |
| CPU Idle            | 0.02%      |
| Swap Usage          | 0 KB       |
| PIDs                | 7          |

**On a typical 32 GB server:**
- Memory usage: ~0.04%
- No swapping
- Plenty of headroom

BZOD runs closer to a lightweight infrastructure service than a typical web application.

# Design Philosophy

BZOD is built around five principles:

## 1. Single Binary Deployment

A production deployment should not require:

* Kubernetes
* Elasticsearch
* Redis
* Multiple microservices

A single binary should be capable of serving the entire platform.

---

## 2. SQLite First

SQLite offers:

* Simplicity
* Reliability
* Easy Backups
* Easy Recovery
* Minimal Operational Overhead

BZOD embraces SQLite instead of treating it as a development-only database.

---

## 3. Self-Hosted Ownership

All data belongs to the operator.

This includes:

* URLs
* Analytics
* User Accounts
* QR Codes
* Landing Pages
* Audit Logs

No telemetry is required.

---

## 4. Multi-Tenant Isolation

Each tenant receives isolated storage and analytics.

The failure or compromise of one tenant must not affect another tenant.

---

## 5. Recoverability

A platform that cannot be restored is not production ready.

BZOD includes:

* Backup Workflows
* Restore Workflows
* Disaster Recovery Procedures
* Upgrade Validation
* Integrity Checks

as first-class features.

---

# Key Features

## URL Management

* Short URLs
* Custom Slugs
* Bulk Operations
* Password-Protected Links
* Expiring Links
* Smart Preview Pages
* QR Code Generation
* QR Downloads (PNG)
* QR Downloads (SVG)

---

## Landing Pages

* Hosted Landing Pages
* Custom Slugs
* QR Support
* Analytics Integration
* Shareable Campaign Pages

---

## Analytics

* Visitor Tracking
* QR Scan Analytics
* Browser Detection
* Referrer Analysis
* Daily Statistics
* Monthly Statistics
* CSV Export
* JSON Export
* Raw Visitor Logs

---

## Multi-User Platform

* User Accounts
* User Quotas
* Session Management
* API Tokens
* Tenant Isolation
* Ownership Validation
* Administrative Controls

---

## Administration

* User Management
* Moderation
* Audit Logs
* Session Administration
* Quota Management
* Backup Management
* Health Dashboard
* Namespace Diagnostics

---

## Operations

* Backup & Restore
* Disaster Recovery
* Health Monitoring
* Upgrade Validation
* Migration Framework
* WAL-Enabled SQLite Databases
* Registry Integrity Validation

---

# Global Namespace Integrity

BZOD enforces a platform-wide slug namespace.

The following resources cannot share the same slug:

* Administrator URLs
* Administrator Landing Pages
* User URLs
* User Landing Pages

Example:

Valid:

```text
/company
/docs
/about
```

Invalid:

```text
Admin URL:
/docs

User Landing Page:
/docs
```

Namespace conflicts are automatically detected during:

* Creation
* Startup
* Restore
* Upgrade
* Validation

This guarantees predictable routing behavior across the platform.

---

# Global Slug Registry

BZOD uses a centralized registry to manage all public routes.

Stored in:

```text
system.db
```

Registry table:

```text
global_slugs
```

Tracks:

* slug
* owner_user_id
* target_type
* target_id
* status

The registry acts as the authoritative source of truth for:

* Redirect Resolution
* Landing Page Routing
* QR Generation
* Ownership Validation
* Namespace Enforcement

---

# Architecture

```text
                           Browser
                               │
                               ▼
                    ┌──────────────────┐
                    │    Axum Server   │
                    └────────┬─────────┘
                             │
          ┌──────────────────┼──────────────────┐
          │                  │                  │
          ▼                  ▼                  ▼

      users.db          system.db        User Databases

      Accounts          Global Slugs     content.db
      Sessions          Moderation       analytics.db
      API Tokens        Audit Events
      Quotas            Settings
```

---

# High-Level Request Flow

```text
Client Request
      │
      ▼
Axum Router
      │
      ▼
Global Slug Registry Lookup
      │
      ▼
Ownership Resolution
      │
      ▼
Tenant Database
      │
      ▼
Response
```

---

# Data Layout

```text
data/
├── admin/
│   ├── users.db
│   ├── system.db
│   ├── admin.db
│   └── admin.db-wal
│
└── users/
    ├── 1/
    │   ├── content.db
    │   ├── analytics.db
    │   └── profile.db
    │
    ├── 2/
    │   ├── content.db
    │   ├── analytics.db
    │   └── profile.db
    │
    └── ...
```

---

# Core Databases

## users.db

Stores:

* Users
* Password Hashes
* Sessions
* API Tokens
* Quotas
* User Status

---

## system.db

Stores:

* Global Slug Registry
* Moderation Events
* Audit Events
* Reserved Slugs
* Settings
* System Metadata

---

## Tenant Databases

Every user receives isolated databases.

### content.db

Stores:

* URLs
* Landing Pages
* Metadata
* QR Relationships
* Preview Data

### analytics.db

Stores:

* Visits
* QR Scans
* Referrers
* User Agents
* Aggregated Statistics

### profile.db

Stores:

* User Preferences
* Tenant Metadata
* Account Configuration

---

# Feature Matrix

| Feature                     | Status |
| --------------------------- | ------ |
| URL Shortening              | ✅      |
| Custom Slugs                | ✅      |
| Landing Pages               | ✅      |
| QR Codes                    | ✅      |
| QR Analytics                | ✅      |
| PNG Downloads               | ✅      |
| SVG Downloads               | ✅      |
| Password Protection         | ✅      |
| Link Expiration             | ✅      |
| Analytics Dashboard         | ✅      |
| CSV Export                  | ✅      |
| JSON Export                 | ✅      |
| REST API                    | ✅      |
| Web UI                      | ✅      |
| CLI                         | ✅      |
| Multi-User Support          | ✅      |
| User Quotas                 | ✅      |
| Session Management          | ✅      |
| API Tokens                  | ✅      |
| Audit Logging               | ✅      |
| Moderation                  | ✅      |
| Global Namespace Integrity  | ✅      |
| Ownership Isolation         | ✅      |
| Dashboard Parity            | ✅      |
| Backup & Restore            | ✅      |
| Disaster Recovery           | ✅      |
| Health Monitoring           | ✅      |
| Upgrade Validation          | ✅      |
| Restore Collision Detection | ✅      |
| Stale Reservation Recovery  | ✅      |

---
# Screenshots

## Administrator Dashboard

Features:

* Platform Statistics
* User Management
* Health Monitoring
* Moderation
* Audit Events
* Namespace Diagnostics

```text
screenshots/dashboard.png
```

---

## URL Management

Features:

* URL Creation
* QR Preview
* PNG Download
* SVG Download
* Analytics
* Export Functions

```text
screenshots/short-url-panel.png
```

---

## Landing Pages

Features:

* Landing Page Creation
* Slug Management
* QR Support
* Analytics
* Public Publishing

```text
screenshots/landing-page-panel.png
```

---

## Settings

Features:

* Password Management
* Session Control
* API Tokens
* User Preferences

```text
screenshots/settings.png
```

---

## Health Dashboard

Features:

* Database Health
* Namespace Validation
* Storage Information
* System Diagnostics

```text
screenshots/server-status.png
```

---

# Installation

BZOD can be deployed using:

* Docker
* Docker Compose
* Native Binary
* Systemd Service

Supported Platforms:

* Linux
* Debian
* Ubuntu
* Arch Linux
* Rocky Linux
* Alma Linux
* Fedora

---

# Docker Deployment

Build:

```bash
docker compose build
```

Start:

```bash
docker compose up -d
```

Check status:

```bash
docker compose ps
```

View logs:

```bash
docker compose logs -f
```

Stop:

```bash
docker compose down
```

---

# Docker Compose Example

```yaml
services:
  bzod:
    build: .
    container_name: bzod
    restart: unless-stopped

    ports:
      - "8654:8654"

    volumes:
      - ./data:/app/data

    environment:
      - BZOD_BASE_URL=https://bzo.in
```

---

# Native Installation

Clone repository:

```bash
git clone https://github.com/thakares/nx9-url-shortener.git
cd nx9-url-shortener
```

Build:

```bash
cargo build --release
```

Binary:

```text
target/release/bzod
```

Run:

```bash
./target/release/bzod serve
```

---

# Systemd Service

Example:

```ini
[Unit]
Description=BZOD URL Management Platform
After=network.target

[Service]
User=bzod
Group=bzod

WorkingDirectory=/opt/bzod

ExecStart=/opt/bzod/bzod serve

Restart=always

[Install]
WantedBy=multi-user.target
```

Enable:

```bash
sudo systemctl enable bzod
sudo systemctl start bzod
```

Status:

```bash
sudo systemctl status bzod
```

---

# Command Line Interface

BZOD includes an extensive command-line interface.

Display help:

```bash
bzod --help
```

Available commands:

```text
serve
backup
restore
migrate
stats
validate
doctor

shorten
expand

create-admin

create-user
delete-user
disable-user
enable-user
reset-password
list-users

backup-user
restore-user
```

---

# Example Commands

Create administrator:

```bash
bzod create-admin
```

Create user:

```bash
bzod create-user
```

List users:

```bash
bzod list-users
```

Disable user:

```bash
bzod disable-user
```

Backup system:

```bash
bzod backup
```

Restore system:

```bash
bzod restore backup.tar.gz
```

Health diagnostics:

```bash
bzod doctor
```

Statistics:

```bash
bzod stats
```

Shorten URL:

```bash
bzod shorten https://example.com
```

Expand URL:

```bash
bzod expand abc123
```

---

# REST API

BZOD provides a RESTful JSON API.

Typical operations:

* Create URLs
* Update URLs
* Delete URLs
* Retrieve Analytics
* Export Data
* Manage Landing Pages

Example:

```http
POST /api/urls
```

```json
{
  "destination": "https://example.com",
  "code": "example"
}
```

Response:

```json
{
  "success": true,
  "code": "example"
}
```

See:

```text
docs/API.md
```

for full API documentation.

---

# Security

Security features include:

* Argon2 Password Hashing
* Session Management
* CSRF Protection
* Ownership Validation
* Tenant Isolation
* Namespace Integrity
* Audit Logging

Users cannot:

* Access other user resources
* Access other user analytics
* Export other user data
* Modify other user records

For details see:

```text
docs/SECURITY.md
```

---

# Backup & Recovery

BZOD treats recoverability as a core feature.

Supported:

* Full System Backup
* Full System Restore
* Per User Backup
* Per User Restore
* Disaster Recovery
* Upgrade Validation
* Collision Detection

See:

```text
docs/BACKUP_RESTORE.md
```

---

# Testing

BZOD includes an extensive automated validation suite.

Validation Categories:

* Unit Tests
* Integration Tests
* HTTP E2E Tests
* Security Tests
* Business Workflow Tests
* Backup Tests
* Restore Tests
* Disaster Recovery Tests
* Upgrade Validation Tests
* Namespace Integrity Tests
* Ownership Isolation Tests
* Dashboard Parity Tests
* QR Endpoint Tests
* Concurrency Tests
* WAL Recovery Tests

Run validation:

```bash
cargo fmt --check

cargo clippy --all-targets -- -D warnings

cargo test --all-targets -- --nocapture
```

Build production release:

```bash
cargo build --release
```

See:

```text
docs/TESTING.md
```

---

# Documentation

| Document          | Description                  |
| ----------------- | ---------------------------- |
| ADMIN_GUIDE.md    | Administrator Operations     |
| API.md            | REST API Reference           |
| ARCHITECTURE.md   | System Architecture          |
| BACKUP_RESTORE.md | Backup & Recovery            |
| CHANGELOG.md      | Version History              |
| CLI.md            | Command Reference            |
| COMPARISON.md     | Comparison With Alternatives |
| DATABASES.md      | Database Architecture        |
| DOCKER-Deploy.md  | Docker Deployment            |
| INSTALL.md        | Installation Guide           |
| MULTI_USER.md     | Multi-Tenant Architecture    |
| RELEASE-NOTES.md  | Release Notes                |
| SECURITY.md       | Security Model               |
| TESTING.md        | Testing Guide                |
| UPGRADE.md        | Upgrade Procedures           |

---

# Project Structure

```text
src/
├── analytics/
├── auth/
├── charts/
├── cli/
├── db/
├── jobs/
├── models/
├── services/
├── templates/
├── utils/
└── web/

templates/
tests/
docs/
www/
```

Current codebase:

```text
~200 files
Rust
SQLite
Axum
Askama
```

---

# Comparison

| Feature                  | BZOD | Traditional URL Shortener |
| ------------------------ | ---- | ------------------------- |
| URL Shortening           | ✅    | ✅                         |
| Landing Pages            | ✅    | ❌                         |
| QR Analytics             | ✅    | Limited                   |
| Multi-User Support       | ✅    | Limited                   |
| Audit Trail              | ✅    | Rare                      |
| Backup & Restore         | ✅    | Rare                      |
| Ownership Isolation      | ✅    | Rare                      |
| Namespace Integrity      | ✅    | Rare                      |
| Health Diagnostics       | ✅    | Rare                      |
| Single Binary Deployment | ✅    | Varies                    |

For detailed comparisons:

```text
docs/COMPARISON.md
```

---

# Release Status

## v0.5.1

Namespace Integrity & Multi-User Hardening Release

Status:

```text
Production Ready
```

Validated:

* Formatting
* Static Analysis
* Release Builds
* Namespace Integrity
* Ownership Isolation
* Dashboard Parity
* QR Endpoints
* Routing
* Upgrade Validation
* Backup & Restore
* Disaster Recovery
* Security Validation

---

# Roadmap

Future areas of exploration:

* SSO Integration
* OIDC Authentication
* LDAP Integration
* Enhanced API Tokens
* Scheduled Reporting
* Additional Export Formats
* Enhanced Moderation Tools
* Advanced Analytics

Roadmap priorities remain guided by:

* Operational Simplicity
* Reliability
* Recoverability
* Self-Hosting

---

# License

Dual Licensed:

* MIT License
* Apache License 2.0

See:

```text
LICENSE-MIT
LICENSE-APACHE
```

---

# Repository

GitHub:

```text
https://github.com/thakares/nx9-url-shortener
```

Codeberg:

```text
https://codeberg.org/thakares/nx9-url-shortener
```

---

# Author

**Sunil Thakare**

BZOD is developed as a practical, self-hosted URL management platform focused on simplicity, ownership, and recoverability.

---

# Final Thoughts

BZOD is not merely a URL shortener.

It is a self-hosted platform for:

* URL Management
* Landing Pages
* QR Analytics
* Multi-Tenant Operations
* Administrative Control
* Data Ownership
* Backup & Recovery

while remaining deployable as a single Rust application backed by SQLite.

The goal is simple:

> Own your links. Own your analytics. Own your data.
