# BZOD

> **Self-hosted Multi-User URL Management Platform with Landing Pages, QR Analytics, Global Namespace Integrity, and Operational Tooling — written in Rust.**

![Rust](https://img.shields.io/badge/Rust-Stable-orange)
![SQLite](https://img.shields.io/badge/SQLite-Embedded-blue)
![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey)
![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-green)
![Version](https://img.shields.io/badge/Version-v0.6.0-purple)

[![GitHub](https://img.shields.io/badge/GitHub-thakares%2Fbzod-181717?logo=github)](https://github.com/thakares/bzod)
[![Codeberg](https://img.shields.io/badge/Codeberg-thakares%2Fbzod-2185D0?logo=codeberg)](https://codeberg.org/thakares/bzod)

BZOD is a modern, lightweight, self-hosted platform for managing short URLs, landing pages, QR codes, analytics, and multi-user deployments.

Unlike traditional URL shorteners, BZOD is designed as a complete URL Management Platform that combines production-ready operational tooling, strong namespace integrity, multi-tenant isolation, and comprehensive administrative capabilities into a single deployable Rust binary powered entirely by SQLite.

Whether you're running a homelab, managing enterprise links, hosting marketing campaigns, operating educational portals, or deploying internal government services, BZOD provides complete ownership of your infrastructure without cloud dependencies or heavyweight external services.

---

## Why BZOD?

Most URL shorteners focus solely on redirects.

BZOD takes a broader approach by combining URL management with operational tooling required for real-world production deployments.

Core capabilities include:

- URL Shortening
- Landing Pages
- QR Code Generation (PNG & SVG)
- QR Analytics
- Visitor Analytics
- Multi-User Platform
- REST API
- Role-Based Access Control (RBAC)
- Global Namespace Registry
- Registry Validation
- Transaction-safe Registry Repair
- Health Diagnostics
- Audit Logging
- Backup & Restore
- User Backup & Restore
- Disaster Recovery
- Upgrade Validation
- Single Binary Deployment

The result is a platform that is easy to deploy, lightweight to operate, and entirely controlled by its owner.

---

## Designed For

BZOD is suitable for:

- 🏠 Homelabs
- 🚀 Startups
- 🏢 Small & Medium Businesses
- 🏭 Enterprise Deployments
- 🎓 Educational Institutions
- 🏛 Government Organizations
- 🌐 Internal Corporate Platforms
- ❤️ Self-hosting Enthusiasts

---

## Complete Ownership

With BZOD, you own everything:

- URLs
- Landing Pages
- QR Codes
- Analytics
- Users
- Sessions
- API Tokens
- Audit Logs
- System Configuration
- Backups

No mandatory cloud services.

No telemetry.

No vendor lock-in.

No recurring subscription fees.

---

## Runtime Efficiency (v0.6.0)

| Metric | Value |
|---------|------:|
| Release Binary | ~11 MB |
| Runtime Memory (RSS) | ~12 MB |
| Peak Memory | ~12 MB |
| Idle CPU Usage | ~0.02% |
| Swap Usage | 0 KB |
| Database | SQLite (WAL) |

Typical deployment on a 32 GB Linux server:

- Memory usage below **0.05%**
- Negligible CPU consumption while idle
- Zero swap usage
- No external infrastructure requirements

BZOD behaves like lightweight infrastructure software rather than a traditional web application.
# Installation

## System Requirements

BZOD is intentionally lightweight and has minimal runtime requirements.

### Minimum

| Component | Requirement |
|-----------|-------------|
| CPU | 1 Core |
| Memory | 512 MB |
| Storage | 100 MB |
| OS | Linux (x86_64) |

### Recommended

| Component | Requirement |
|-----------|-------------|
| CPU | 2+ Cores |
| Memory | 2 GB |
| Storage | 5 GB SSD |
| Database | SQLite (WAL) |

---

# Deployment Options

BZOD supports multiple deployment methods.

- Docker
- Docker Compose
- Native Linux Binary
- Systemd Service
- Reverse Proxy (Nginx, Caddy, Traefik, NPM)

No Kubernetes is required.

---

# Docker

Example:

```yaml
services:
  bzod:
    image: ghcr.io/thakares/bzod:latest
    container_name: bzod

    restart: unless-stopped

    ports:
      - "8654:8654"

    volumes:
      - ./data:/app/data

    environment:
      - RUST_LOG=info
```

Start:

```bash
docker compose up -d
```

---

# Native Installation

Download the latest release.

```bash
chmod +x bzod

sudo mv bzod /usr/local/bin/
```

Verify:

```bash
bzod --help
```

---

# Initialize

Create the administrator.

```bash
bzod create-admin
```

Start the server.

```bash
bzod serve
```

Default server:

```
http://localhost:8654
```

---

# Systemd Service

Example:

```ini
[Unit]
Description=BZOD URL Management Platform

After=network.target

[Service]

ExecStart=/usr/local/bin/bzod serve

Restart=always

User=bzod

WorkingDirectory=/opt/bzod

[Install]

WantedBy=multi-user.target
```

Enable:

```bash
sudo systemctl enable bzod

sudo systemctl start bzod
```

---

# Reverse Proxy

BZOD works behind:

- Nginx
- Caddy
- Apache
- Traefik
- Nginx Proxy Manager

TLS termination should be handled by the reverse proxy.

---

# CLI Overview

```text
bzod serve
bzod backup
bzod restore
bzod migrate
bzod stats
bzod validate
bzod doctor
bzod repair

bzod shorten
bzod expand

bzod create-admin

bzod create-user
bzod delete-user
bzod disable-user
bzod enable-user
bzod reset-password
bzod list-users

bzod backup-user
bzod restore-user

bzod admin-migrate
```

---

# Health Diagnostics

Inspect platform health.

```bash
bzod doctor
```

Checks include:

- SQLite Integrity
- WAL Status
- Foreign Keys
- Registry Integrity
- Namespace Consistency
- Missing Databases
- Missing Records
- Ownership Validation

---

# Registry Repair

Preview repairs.

```bash
bzod repair registry --dry-run
```

Repair all orphaned entries.

```bash
bzod repair registry --force
```

Repair a single slug.

```bash
bzod repair registry \
    --slug my-page \
    --force
```

Repairs are:

- Transaction Safe
- Atomic
- Non-destructive
- Fully Logged

---

# Backup

Platform Backup

```bash
bzod backup
```

Restore

```bash
bzod restore backup.tar.gz
```

---

# User Backup

Backup a single tenant.

```bash
bzod backup-user \
    user@example.com
```

Restore.

```bash
bzod restore-user \
    backup.tar.zst
```

---

# Statistics

View platform statistics.

```bash
bzod stats
```

Displays:

- Users
- URLs
- Landing Pages
- QR Codes
- Analytics
- Databases
- Storage Usage

---

# URL Management

Create a short URL.

```bash
bzod shorten \
    https://example.com
```

Expand a short code.

```bash
bzod expand abc123
```

---

# Administrator Commands

Create Administrator

```bash
bzod create-admin
```

List Users

```bash
bzod list-users
```

Disable User

```bash
bzod disable-user alice
```

Enable User

```bash
bzod enable-user alice
```

Delete User

```bash
bzod delete-user alice
```

Reset Password

```bash
bzod reset-password alice
```

---

# Admin Migration

Preview migration.

```bash
bzod admin-migrate 2 --dry-run
```

Execute migration.

```bash
bzod admin-migrate 2 --force
```

---

# REST API

BZOD provides a REST API for automation.

Examples include:

- URL Management
- Landing Pages
- Analytics
- QR Codes
- Administration
- User Management

Authentication uses API Tokens.

---

# Security

Security features include:

- Argon2 Password Hashing
- Secure Cookies
- API Tokens
- RBAC
- Tenant Isolation
- Audit Logging
- Ownership Validation
- Namespace Validation
- SQLite Foreign Keys
- Transaction-safe Repairs

---

# Backup & Disaster Recovery

Designed for long-term reliability.

Features include:

- Platform Backup
- Tenant Backup
- Restore Validation
- Upgrade Validation
- Registry Validation
- Registry Repair
- Backup Manifest
- WAL Recovery

---

# Documentation

Additional documentation is available in the `docs/` directory.

- ADMIN_GUIDE.md
- ARCHITECTURE.md
- BACKUP_RESTORE.md
- CHANGELOG.md
- CLI.md
- COMPARISON.md
- DATABASES.md
- INSTALL.md
- MULTI_USER.md
- RELEASE-NOTES.md
- SECURITY.md
- TESTING.md
- UPGRADE.md
# Platform Architecture

BZOD is organized into independent functional layers, keeping the core lightweight while allowing future expansion.

```text
                    Browser / API Client
                            │
                            ▼
                  ┌────────────────────┐
                  │     Axum Router    │
                  └─────────┬──────────┘
                            │
        ┌───────────────────┼───────────────────┐
        ▼                   ▼                   ▼

   Authentication      Business Logic      REST API

        │                   │                   │
        └──────────────┬────┴───────────────────┘
                       ▼

             Global Slug Registry

                       │
                       ▼

             Tenant Resolution Layer

                       │
        ┌──────────────┴───────────────┐
        ▼                              ▼

    Administrator                 Standard Users

        │                              │

        ▼                              ▼

 admin/*.db                 users/<id>/*.db
```

---

# Multi-Tenant Architecture

BZOD is designed around complete tenant isolation.

Each user owns independent databases.

```
data/

├── admin/
│   ├── admin.db
│   ├── users.db
│   └── system.db
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

Benefits include:

- Complete tenant isolation
- Independent backups
- Faster restores
- Simplified migrations
- Better security
- Reduced blast radius

---

# Global Namespace Registry

Unlike many URL shorteners, BZOD guarantees that every public slug is globally unique.

Examples:

```
/docs
/about
/company
/presentation
```

cannot simultaneously exist as:

- Administrator URL
- Administrator Landing Page
- User URL
- User Landing Page

The registry is enforced during:

- Creation
- Updates
- Import
- Restore
- Migration
- Startup validation

This guarantees deterministic routing.

---

# Registry Validator

The Registry Validator introduced in v0.5.3 is responsible for maintaining namespace integrity.

It validates:

- Missing URLs
- Missing Landing Pages
- Missing Databases
- Invalid Owners
- Orphaned Registry Entries
- Stale Reservations
- Misplaced Administrator Content

Every validation produces structured results used by multiple platform components.

---

# Registry Repair Framework

The Registry Repair Framework provides explicit repair operations.

Workflow:

```
Scan
 ↓
Validate
 ↓
Preview
 ↓
Transaction
 ↓
Repair
 ↓
Verify
```

Supported commands:

```bash
bzod repair registry --dry-run

bzod repair registry --force

bzod repair registry --slug my-page --dry-run

bzod repair registry --slug my-page --force
```

Characteristics:

- Transaction-safe
- Atomic
- Idempotent
- Fully logged
- Read-only preview
- Explicit administrator confirmation

---

# Health Diagnostics

The Doctor subsystem continuously validates platform health.

Checks include:

## Database Health

- SQLite Integrity
- WAL Mode
- Foreign Keys
- Schema Version

---

## Namespace Health

- Registry Consistency
- Ownership Validation
- Missing Records
- Duplicate Entries

---

## Operational Health

- Tenant Databases
- Administrator Databases
- Registry References
- Restore Compatibility

Run:

```bash
bzod doctor
```

---

# Security Model

BZOD follows a defense-in-depth approach.

Authentication

- Username & Password
- Argon2 Password Hashing
- Secure Cookies
- API Tokens

Authorization

- Role-Based Access Control
- Administrator Privileges
- Tenant Isolation

Validation

- Namespace Validation
- Ownership Validation
- Restore Validation
- Registry Validation

Database

- SQLite Foreign Keys
- WAL Mode
- Atomic Transactions

Operations

- Audit Logging
- Registry Repair
- Backup Verification

---

# Role-Based Access Control (RBAC)

Two primary account types exist.

## Administrator

Capabilities:

- Platform Management
- User Management
- Global Analytics
- Moderation
- Registry Repair
- Backups
- Restore
- Audit Logs
- System Statistics

---

## Standard User

Capabilities:

- Personal URLs
- Personal Landing Pages
- Personal Analytics
- QR Downloads
- Profile Management

Users cannot access platform-wide administrative data.

---

# REST API

BZOD includes a REST API suitable for automation.

Current capabilities include:

- URL Management
- Landing Pages
- Analytics
- QR Codes
- Authentication
- Administration

Future API expansions are planned for:

- Webhooks
- Batch Operations
- Tenant Statistics
- OpenAPI Documentation

---

# Backup & Restore

Backups are first-class features.

Platform Backup

```bash
bzod backup
```

Platform Restore

```bash
bzod restore backup.tar.gz
```

User Backup

```bash
bzod backup-user alice
```

User Restore

```bash
bzod restore-user alice-backup.tar.zst
```

Features:

- Backup Manifest
- Validation
- Integrity Checks
- Restore Verification
- Automatic Database Migration
- WAL Compatibility

---

# Testing

BZOD includes extensive automated testing.

Validation is performed using:

```bash
cargo fmt --check

cargo clippy --all-targets -- -D warnings

cargo test --all-targets -- --nocapture

cargo build --release
```

Test coverage includes:

- Authentication
- Authorization
- Routing
- Namespace Integrity
- Registry Validation
- Registry Repair
- Backup & Restore
- User Isolation
- Concurrency
- QR Endpoints
- Analytics
- Moderation
- Business Workflows
- Database Integrity
- Transactions

---

# Project Structure

```
src/

├── auth/
├── cli/
├── db/
├── jobs/
├── middleware/
├── services/
├── templates/
├── utils/
├── web/
│   ├── admin/
│   │   ├── analytics.rs
│   │   ├── api_keys.rs
│   │   ├── audit.rs
│   │   ├── auth.rs
│   │   ├── backups.rs
│   │   ├── dashboard.rs
│   │   ├── health.rs
│   │   ├── moderation.rs
│   │   ├── mod.rs
│   │   ├── pages.rs
│   │   ├── quotas.rs
│   │   ├── sessions.rs
│   │   ├── settings.rs
│   │   ├── urls.rs
│   │   └── users.rs
│   ├── api.rs
│   ├── bulk.rs
│   ├── middleware.rs
│   ├── mod.rs
│   ├── multi_user.rs
│   ├── pages.rs
│   ├── password_gate.rs
│   ├── qr.rs
│   ├── redirect.rs
│   ├── routes.rs
│   └── system.rs

templates/

tests/

docs/

www/
```

The codebase is organized by responsibility, making it straightforward to extend without introducing unnecessary complexity.

---

# Documentation

The project includes comprehensive documentation.

| Document | Description |
|----------|-------------|
| ADMIN_GUIDE.md | Administrator Guide |
| ARCHITECTURE.md | Internal Architecture |
| BACKUP_RESTORE.md | Backup & Recovery |
| CHANGELOG.md | Project History |
| CLI.md | Command Reference |
| COMPARISON.md | Feature Comparison |
| DATABASES.md | Database Layout |
| INSTALL.md | Installation Guide |
| MULTI_USER.md | Multi-User Architecture |
| RELEASE-NOTES.md | Release History |
| SECURITY.md | Security Model |
| TESTING.md | Testing Guide |
| UPGRADE.md | Upgrade Instructions |

---

# Performance

Typical production deployment:

| Metric | Value |
|---------|-------:|
| Binary Size | ~11 MB |
| Memory Usage | ~12 MB RSS |
| Database | SQLite (WAL) |
| External Dependencies | None |

BZOD scales efficiently for:

- Personal deployments
- Small businesses
- Enterprise internal services
- Educational institutions
- Government organizations

---

# Comparison

| Feature | BZOD | Traditional URL Shorteners |
|---------|:----:|:--------------------------:|
| Rust | ✅ | Rare |
| SQLite Only | ✅ | ❌ |
| Single Binary | ✅ | ❌ |
| Multi-User | ✅ | Limited |
| Landing Pages | ✅ | Limited |
| QR PNG/SVG | ✅ | Limited |
| Analytics | ✅ | Basic |
| Global Namespace | ✅ | Rare |
| Registry Validator | ✅ | ❌ |
| Registry Repair | ✅ | ❌ |
| Health Diagnostics | ✅ | ❌ |
| Backup & Restore | ✅ | Limited |
| User Backup | ✅ | ❌ |
| Transaction-safe Repairs | ✅ | ❌ |
| Self Hosted | ✅ | Mixed |
| Zero External Services | ✅ | Rare |

---

# Production Ready

BZOD has been designed for production deployments with emphasis on:

- Operational simplicity
- Reliability
- Maintainability
- Recoverability
- Security
- Performance

It provides enterprise-grade operational tooling while remaining lightweight enough for homelab deployments.

---
# Roadmap

BZOD follows a pragmatic roadmap focused on operational reliability, maintainability, and long-term ownership.

Rather than chasing feature count, development emphasizes quality, stability, and self-hosting excellence.

---

## Completed

### v0.1

- Basic URL Shortener
- SQLite Storage
- Web Interface

---

### v0.2

- QR Code Generation
- Password Protected URLs
- Link Expiration
- Preview Pages
- Bulk Operations
- Audit Events

---

### v0.3

- Landing Pages
- Analytics
- User Dashboard
- Administrator Dashboard
- QR Downloads

---

### v0.4

- Multi-User Platform
- Tenant Isolation
- REST API
- User Administration
- Backup & Restore
- Moderation
- User Quotas

---

### v0.5

Major operational improvements.

Highlights include:

- Global Namespace Registry
- Routing Integrity
- Dashboard Parity
- Administrator Content Consistency
- Registry Validator
- Registry Repair Framework
- RBAC
- Health Diagnostics
- Disaster Recovery
- Upgrade Validation
- Backup Manifest
- Transaction-safe Maintenance
- Modular Admin Architecture (v0.6.0)
- Redirect Handler Hardening (v0.6.0)

---

# Future Roadmap

## v0.6

Planned features include:

- Webhooks
- OpenAPI Documentation
- API Versioning
- Personal Statistics API
- Improved Search
- Bulk Import
- Bulk Export
- Custom Domains
- Better QR Styling

---

## v0.7

Planned improvements:

- Email Notifications
- Team Workspaces
- Shared Projects
- Administrator Notifications
- Enhanced Moderation
- Scheduled Jobs

---

## Long-Term Vision

Potential future capabilities:

- SSO / OpenID Connect
- LDAP Integration
- High Availability
- Read-only Replicas
- Plugin System
- Metrics Export (Prometheus)
- Event Streaming
- Federation
- Object Storage Support

Development priorities will continue to favor reliability over unnecessary complexity.

---

# Why Rust?

Rust provides several advantages for long-running infrastructure services.

- Memory Safety
- Zero-Cost Abstractions
- Excellent Performance
- Strong Concurrency
- Predictable Resource Usage
- Single Binary Deployment
- Cross-Platform Support

These characteristics make Rust particularly well suited for self-hosted infrastructure software.

---

# Why SQLite?

BZOD intentionally uses SQLite as its primary database.

Benefits include:

- Embedded Database
- Zero Configuration
- ACID Transactions
- WAL Support
- Excellent Performance
- Easy Backups
- Simple Disaster Recovery
- Proven Reliability

For the majority of deployments, SQLite offers an excellent balance between performance and operational simplicity.

---

# Contributing

Contributions are welcome.

Areas where contributions are particularly valuable include:

- Bug Reports
- Feature Requests
- Documentation
- Performance Improvements
- Security Reviews
- Testing
- Translations

Before submitting large changes, please open an issue to discuss the proposed implementation.

---

# Reporting Issues

Please include as much information as possible.

Useful details include:

- BZOD Version
- Operating System
- Deployment Method
- Browser
- Logs
- Reproduction Steps

This helps reproduce and resolve issues efficiently.

---

# License

Licensed under either of the following, at your option:

- MIT License
- Apache License 2.0

See the LICENSE files for full details.

---

# Repository

GitHub

https://github.com/thakares/bzod

Codeberg

https://codeberg.org/thakares/bzod

---

# Documentation

Complete documentation is available in the `docs/` directory.

- ADMIN_GUIDE.md
- ARCHITECTURE.md
- BACKUP_RESTORE.md
- CHANGELOG.md
- CLI.md
- COMPARISON.md
- DATABASES.md
- INSTALL.md
- MULTI_USER.md
- RELEASE-NOTES.md
- SECURITY.md
- TESTING.md
- UPGRADE.md

---

# Support

If you find BZOD useful:

- ⭐ Star the project
- 🐞 Report issues
- 💡 Suggest improvements
- 📖 Improve documentation
- 🚀 Share the project

Community feedback helps guide future development.

---

# Project Status

**Current Version**

**v0.5.3**

Production Ready

### Highlights

- ✅ Single Rust Binary
- ✅ SQLite + WAL
- ✅ Multi-User Architecture
- ✅ Global Namespace Registry
- ✅ Landing Pages
- ✅ QR Code Generation
- ✅ PNG & SVG Downloads
- ✅ Analytics
- ✅ REST API
- ✅ Role-Based Access Control
- ✅ Registry Validator
- ✅ Registry Repair Framework
- ✅ Health Diagnostics
- ✅ Backup & Restore
- ✅ Disaster Recovery
- ✅ Upgrade Validation
- ✅ Extensive Automated Test Suite

---

# Project Philosophy

BZOD is more than a URL shortener.

It is a lightweight, self-hosted URL Management Platform designed around ownership, reliability, and operational excellence.

Every feature is guided by a few core principles:

- Own your infrastructure.
- Own your data.
- Keep deployments simple.
- Prefer reliability over complexity.
- Build tools that administrators can trust.

The goal is not to become the largest URL management platform, but to become one of the most dependable, maintainable, and resource-efficient self-hosted solutions available.

---

## Acknowledgements

BZOD is built using outstanding open-source software, including:

- Rust
- Axum
- Tokio
- SQLite
- Askama
- Argon2
- QRCode
- Image
- Chrono
- Serde

Many thanks to the Rust and open-source communities whose work makes projects like BZOD possible.

---

# Author

**Sunil Thakare**

GitHub: https://github.com/thakares

Codeberg: https://codeberg.org/thakares

---

# BZOD

**Own your links.  
Own your analytics.  
Own your infrastructure.**