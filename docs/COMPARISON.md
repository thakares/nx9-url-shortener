# BZOD v0.5.0 vs Self-Hosted URL Management Platforms

BZOD is a modern, privacy-focused, self-hosted URL Management Platform written in Rust and developed as part of the NX9 Platform.

Unlike traditional URL shorteners that focus primarily on URL redirection, BZOD provides a complete platform for managing URLs, landing pages, analytics, users, permissions, backups, and operational workflows.

## Quick Comparison

| Feature                  | BZOD | Shlink | YOURLS | Chhoto URL |
|--------------------------|------|--------|--------|------------|
| Language                 | Rust | PHP    | PHP    | Rust       |
| Single Binary            | ✅    | ❌      | ❌      | ✅          |
| Landing Pages            | ✅    | ❌      | Plugin | ❌          |
| QR Code + Analytics      | ✅    | Partial| Plugin | Partial    |
| Password Protection      | ✅    | Limited| Plugin | ❌          |
| Backup & Restore         | ✅    | External| External| ❌         |
| Audit Trail              | ✅    | Limited| Plugin | ❌          |
| CLI Tools                | ✅    | Limited| Limited| Limited    |
| Dependencies             | None | PHP + DB | PHP + DB | None     |
| Deployment Complexity    | Low  | Medium | High   | Low        |

---

# Executive Summary

BZOD combines:

* URL shortening
* Landing pages
* QR code generation
* QR analytics
* Link analytics
* Password-protected links
* Link expiration
* REST API
* Administrative dashboard
* Multi-user operation
* User management
* User quotas
* Session management
* Audit logging
* Moderation
* Backup & restore
* Disaster recovery tooling

into a single Rust binary deployment.

---

# At a Glance

| Feature              | BZOD              |
| -------------------- | ----------------- |
| Language             | Rust              |
| License              | MIT OR Apache-2.0 |
| Deployment           | Single Binary     |
| Runtime Dependencies | None              |
| Database             | SQLite            |
| Multi-User           | Yes               |
| Landing Pages        | Yes               |
| QR Codes             | Yes               |
| Analytics            | Yes               |
| REST API             | Yes               |
| CLI Tools            | Yes               |
| Backups              | Built-in          |
| Audit Logs           | Built-in          |
| RBAC                 | Built-in          |

---

# What Changed in v0.5.0

BZOD v0.5.0 introduces a major architectural evolution.

## New Platform Capabilities

* Multi-user architecture
* Tenant isolation
* Global slug namespace
* User management
* User quotas
* Session management
* Administrative dashboards
* User self-service dashboards
* Audit event logging
* Moderation workflows
* Backup management
* Health monitoring
* Upgrade framework
* Migration tooling

BZOD is no longer merely a URL shortener.

It is now a self-hosted URL Management Platform.

---

# Traditional URL Shortener Comparison

| Capability          | BZOD | Shlink   | YOURLS   | Chhoto URL |
| ------------------- | ---- | -------- | -------- | ---------- |
| URL Shortening      | ✅    | ✅        | ✅        | ✅          |
| Landing Pages       | ✅    | ❌        | Plugin   | ❌          |
| QR Generation       | ✅    | Partial  | Plugin   | Partial    |
| QR Analytics        | ✅    | Partial  | Plugin   | ❌          |
| Password Protection | ✅    | Limited  | Plugin   | ❌          |
| Link Expiration     | ✅    | ✅        | Plugin   | Limited    |
| REST API            | ✅    | ✅        | ✅        | JSON-RPC   |
| Backup & Restore    | ✅    | External | External | ❌          |
| Audit Logs          | ✅    | Limited  | Plugin   | ❌          |
| Multi User          | ✅    | Partial  | Plugin   | ❌          |
| User Quotas         | ✅    | ❌        | ❌        | ❌          |
| User Isolation      | ✅    | ❌        | ❌        | ❌          |
| User Dashboards     | ✅    | ❌        | ❌        | ❌          |

---

# Multi-User Platform Comparison

BZOD v0.5.0 introduces first-class multi-user support.

| Capability             | BZOD |
| ---------------------- | ---- |
| User Accounts          | ✅    |
| Administrator Accounts | ✅    |
| User Isolation         | ✅    |
| User Quotas            | ✅    |
| Session Management     | ✅    |
| API Tokens             | ✅    |
| Audit Trail            | ✅    |
| Moderation             | ✅    |
| Tenant Analytics       | ✅    |
| Self-Service Portal    | ✅    |

Most self-hosted URL shorteners are fundamentally single-user applications.

BZOD is designed for:

* Individuals
* Teams
* Organizations
* Educational Institutions
* Governments
* Service Providers

---

# Security Comparison

| Security Feature          | BZOD | Typical URL Shortener |
| ------------------------- | ---- | --------------------- |
| Argon2id Password Hashing | ✅    | Varies                |
| Session Management        | ✅    | Basic                 |
| CSRF Protection           | ✅    | Varies                |
| RBAC                      | ✅    | Rare                  |
| Audit Logging             | ✅    | Rare                  |
| User Disablement          | ✅    | Rare                  |
| Moderation Controls       | ✅    | Rare                  |
| Tenant Isolation          | ✅    | Rare                  |
| API Token Security        | ✅    | Varies                |

---

# Operations Comparison

| Operational Feature | BZOD |
| ------------------- | ---- |
| Backup Creation     | ✅    |
| Backup Restore      | ✅    |
| User Backup         | ✅    |
| User Restore        | ✅    |
| Disaster Recovery   | ✅    |
| Upgrade Validation  | ✅    |
| Health Monitoring   | ✅    |
| WAL Recovery        | ✅    |
| Migration Framework | ✅    |

Most competing products rely on external tooling for these capabilities.

---

# Deployment Comparison

| Requirement                | BZOD | Shlink   | YOURLS   |
| -------------------------- | ---- | -------- | -------- |
| Single Binary              | ✅    | ❌        | ❌        |
| SQLite Only                | ✅    | Optional | Optional |
| External Database Required | ❌    | Usually  | Usually  |
| Docker Support             | ✅    | ✅        | ✅        |
| Systemd Support            | ✅    | Manual   | Manual   |
| Backup Framework           | ✅    | ❌        | ❌        |
| Upgrade Framework          | ✅    | ❌        | ❌        |

---

# BZOD vs Go-Based URL Shorteners

Popular Go alternatives include:

* Krtk
* Goshorly
* Slash
* Shortr
* Custom Gin/Echo implementations

### Strengths of Go Projects

* Small binaries
* Excellent performance
* Simple codebases

### Strengths of BZOD

* Multi-user support
* Landing pages
* User management
* Built-in analytics
* Backup framework
* Audit logging
* Moderation
* Administrative dashboards

---

# BZOD vs Python-Based Solutions

Examples:

* Pygmy
* Schort
* ReducePy
* Flask-based projects
* FastAPI-based projects

### Python Advantages

* Rapid development
* Familiar ecosystem

### BZOD Advantages

* No runtime dependency
* Lower memory consumption
* Single binary deployment
* Operational tooling included
* Better long-term maintenance characteristics

---

# Reliability & Testing

BZOD v0.5.0 includes a comprehensive automated validation suite.

Coverage includes:

* Unit tests
* Integration tests
* HTTP E2E tests
* Business workflow tests
* Upgrade validation tests
* Backup/restore tests
* Disaster recovery tests
* Security tests
* Concurrency tests
* WAL recovery tests

The platform is validated using more than 90 automated tests.

---

# NX9 Platform Philosophy

BZOD follows the NX9 engineering philosophy:

* Linux-first
* Rust-first
* Self-hosted
* Privacy-first
* No telemetry
* No vendor lock-in
* No external dependencies
* Single binary deployment

The goal is simple:

> Build software that remains useful, understandable, maintainable, and deployable decades into the future.

---

# Who Should Use BZOD?

BZOD is suitable for:

### Individuals

* Personal URL management
* Homelabs
* Self-hosted services

### Organizations

* Marketing campaigns
* Internal redirects
* Landing page hosting

### Governments

* Public service redirects
* Long-term link preservation
* Controlled infrastructure

### Service Providers

* Multi-tenant URL management
* Managed short-link services
* White-label deployments

---

# Conclusion

BZOD v0.5.0 is not simply a URL shortener.

It is a self-hosted URL Management Platform providing:

* Multi-user operation
* Tenant isolation
* URL shortening
* Landing pages
* QR generation
* Analytics
* Audit logging
* Moderation
* User administration
* Backup & restore
* Health monitoring

within a single Rust binary deployment.

BZOD is designed for individuals, organizations, governments, educational institutions, and service providers that require full ownership of their links, analytics, and infrastructure.

> Own your links.
> Own your data.
> Own your infrastructure.

No telemetry. No vendor lock-in. No unnecessary complexity.
