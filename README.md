# BZOD

> Self-hosted Multi-User URL Management Platform written in Rust.

BZOD combines URL shortening, landing pages, QR codes, analytics, moderation, audit logging, backup & restore workflows, and tenant isolation into a single deployable binary powered entirely by SQLite.

Designed for homelabs, organizations, businesses, educational institutions, and government agencies that require complete ownership of their links, analytics, and operational data.

---

## Why BZOD?

Most URL shorteners focus only on redirects and click tracking.

BZOD is designed as a complete self-hosted platform with:

* Multi-user architecture
* Tenant isolation
* Landing pages
* QR code generation
* Analytics
* Audit logging
* Moderation
* Backup & restore
* Disaster recovery
* Administrative tooling
* REST API
* Web UI
* CLI management

All without requiring PostgreSQL, Redis, Elasticsearch, Kubernetes, or external SaaS services.

---

## Key Features

### URL Management

* Short URLs
* Custom slugs
* Bulk operations
* Password-protected links
* Expiring links
* Smart preview pages
* QR code generation

### Landing Pages

* Hosted landing pages
* Custom slugs
* Analytics
* QR code support

### Analytics

* Visitor tracking
* QR scan analytics
* Browser detection
* Referrer analysis
* Daily and monthly statistics
* CSV export
* JSON export
* Raw visitor logs

### Multi-User Platform

* User accounts
* User quotas
* Session management
* API tokens
* Tenant isolation
* Administrative controls

### Administration

* User management
* Moderation
* Audit logs
* Session administration
* Quota management
* Backup management
* Health dashboard

### Operations

* Backup & restore
* Disaster recovery
* Health monitoring
* Upgrade migrations
* WAL-enabled SQLite databases

---

## Architecture

```text
                     ┌─────────────┐
                     │   Browser   │
                     └──────┬──────┘
                            │
                    ┌───────▼───────┐
                    │  Axum Server  │
                    └───────┬───────┘
                            │
       ┌────────────────────┼────────────────────┐
       │                    │                    │
       ▼                    ▼                    ▼
  users.db             system.db          User Databases
  accounts             global_slugs       content.db
  sessions             moderation         analytics.db
  quotas               audit logs         tenant data
  api tokens           settings
```

### Data Layout

```text
data/
├── system.db
├── users.db
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
    └── ...
```

### Core Databases

#### users.db

Stores:

* Users
* Password hashes
* Sessions
* API tokens
* Quotas

#### system.db

Stores:

* Global slug namespace
* Moderation events
* Audit events
* Reserved slugs
* Settings

#### Tenant Databases

Each user receives isolated databases:

##### content.db

* URLs
* Landing pages
* Metadata

##### analytics.db

* Visits
* QR scans
* Referrers
* User agents
* Aggregated statistics

---

## Feature Matrix

| Feature             | Status |
| ------------------- | ------ |
| URL Shortening      | ✅      |
| Custom Slugs        | ✅      |
| Landing Pages       | ✅      |
| QR Codes            | ✅      |
| QR Analytics        | ✅      |
| Password Protection | ✅      |
| Link Expiration     | ✅      |
| Analytics Dashboard | ✅      |
| CSV Export          | ✅      |
| JSON Export         | ✅      |
| REST API            | ✅      |
| Web UI              | ✅      |
| CLI                 | ✅      |
| Multi-User Support  | ✅      |
| User Quotas         | ✅      |
| Session Management  | ✅      |
| API Tokens          | ✅      |
| Moderation          | ✅      |
| Audit Logging       | ✅      |
| Backup & Restore    | ✅      |
| Disaster Recovery   | ✅      |
| Health Monitoring   | ✅      |
| Upgrade Migrations  | ✅      |

---

## Screenshots

### Dashboard

![Dashboard](screenshots/dashboard.png)

### URL Management

![URL Management](screenshots/short-url-panel.png)

### Landing Pages

![Landing Pages](screenshots/landing-page-panel.png)

### Settings

![Settings](screenshots/settings.png)

### Health Dashboard

![Health Dashboard](screenshots/server-status.png)

---

## Installation

### Docker

```bash
docker compose up -d
```

### Native

```bash
cargo build --release
./target/release/bzod serve
```

---

## CLI

### Administration

```bash
bzod create-admin
bzod create-user
bzod delete-user
bzod disable-user
bzod enable-user
bzod reset-password
bzod list-users
```

### Backup & Recovery

```bash
bzod backup
bzod restore
```

### Maintenance

```bash
bzod doctor
bzod migrate
bzod validate
bzod stats
```

---

## Testing

BZOD v0.5.0 includes a comprehensive automated validation suite.

### Validation Coverage

* Unit tests
* Integration tests
* HTTP E2E tests
* Authentication tests
* Migration tests
* Upgrade validation tests
* User isolation tests
* Security tests
* Backup/restore tests
* Disaster recovery tests
* Concurrency tests
* Business workflow tests
* WAL recovery tests

### Execute

```bash
cargo test
```

### Quality Gates

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
cargo audit
```

---

## Documentation

Additional documentation is available in `docs/`.

| Document         | Description                  |
| ---------------- | ---------------------------- |
| API.md           | REST API reference           |
| CHANGELOG.md     | Version history              |
| COMPARISON.md    | Comparison with alternatives |
| DOCKER-Deploy.md | Docker deployment guide      |
| RELEASE-NOTES.md | Release information          |
| TESTING.md       | Validation and testing       |

---

## Release Status

### v0.5.0

General Availability (GA)

Validation completed:

* Formatting
* Linting
* Build verification
* Unit tests
* Integration tests
* HTTP E2E tests
* Business workflow tests
* Upgrade validation tests

---

## Roadmap

Planned future enhancements:

* Geographic analytics
* SSO integration
* OpenAPI specification
* Multi-organization support
* Advanced analytics dashboards
* Background scheduler UI

---

## License

Dual licensed under:

* Apache License 2.0
* MIT License

at your option.

See:

* LICENSE-APACHE
* LICENSE-MIT

---

## Author

Sunil Purushottam Thakare

Built with Rust, SQLite, Axum, Askama, and a preference for simple, maintainable, self-hosted software.
