# BZOD

**A lightweight, self-hosted URL management platform written in Rust.**

BZOD combines URL shortening, QR code generation, password-protected links, smart preview pages, analytics, audit logging, and lifecycle management into a single self-hosted application with zero external dependencies.

Built with Rust, SQLite, Axum, and Askama, BZOD is designed for individuals, organizations, homelab operators, and businesses that want complete control over their links, analytics, and branding.

---

## Highlights

### v0.2.0

* QR code generation (PNG and SVG)
* QR scan analytics
* Password-protected links
* Smart preview pages
* Link expiration
* Audit trail
* Bulk operations
* Expanded test coverage
* Improved health monitoring

---

## Feature Matrix

| Feature                   | Status |
| ------------------------- | ------ |
| URL Shortening            | ✅      |
| Landing Pages             | ✅      |
| QR Code Generation        | ✅      |
| QR Analytics              | ✅      |
| Password-Protected Links  | ✅      |
| Smart Preview Pages       | ✅      |
| Link Expiration           | ✅      |
| One-Time Links            | ✅      |
| Audit Trail               | ✅      |
| Bulk Operations           | ✅      |
| Analytics Dashboard       | ✅      |
| Health Monitoring         | ✅      |
| REST API                  | ✅      |
| CSV Import/Export         | 🚧     |
| Geo Analytics             | 🚧     |
| Multi-User Administration | 🚧     |
| SSO                       | 🚧     |

---

## Features

### URL Shortening

Create short links using compact hexadecimal identifiers.

Example:

```text
https://bzo.in/1bb170
```

Redirects to:

```text
https://very-long-domain-name.com
```

---

### QR Code Generation

Generate QR codes for every short URL.

Supported formats:

* PNG
* SVG

Features:

* Downloadable QR assets
* QR scan analytics
* Bulk QR export
* Print-friendly SVG output

---

### Password-Protected Links

Protect sensitive links using Argon2id-hashed passwords.

Features:

* Password gate
* Secure session handling
* Configurable protection
* Audit logging

---

### Smart Preview Pages

Display branded preview pages before redirecting.

Features:

* Custom title
* Description
* Logo support
* Open Graph metadata
* Social sharing previews

---

### Landing Pages

Create standalone landing pages using dedicated page identifiers.

Example:

```text
https://bzo.in/p/1a2b
```

---

### Link Lifecycle Management

Control link validity.

Features:

* Expiration dates
* Automatic expiry jobs
* One-time links
* Maximum access limits
* Administrative disabling

---

### Analytics

Track:

* Total visits
* QR scans
* Country statistics
* Referrers
* User agents
* Daily statistics
* Monthly statistics
* Yearly statistics

---

### Audit Trail

Track administrative actions including:

* Login
* Logout
* URL creation
* URL updates
* URL deletion
* QR operations
* Configuration changes

---

### Administrative Dashboard

Web-based administration interface featuring:

* URL management
* QR code management
* Landing page management
* Preview page management
* API token management
* Audit logs
* Link expiration controls
* Health monitoring
* Analytics dashboard
* SVG charts
* Bulk operations

---

### API Support

REST API endpoints for automation and integration.

```text
/api/v1/*
```

Supports:

* URL creation
* URL management
* QR generation
* Analytics access
* Bulk operations

---

### Security

* Password-protected administration interface
* Password-protected links
* Argon2id password hashing
* Session management
* CSRF protection
* API token authentication
* Audit logging
* Link access controls

---

### Self-Hosted

No external services required.

Dependencies:

* Rust
* SQLite
* Docker (optional)

No:

* React
* Node.js
* Redis
* PostgreSQL
* MongoDB
* Kubernetes
* External SaaS

---

## Architecture

### Databases

BZOD uses four SQLite databases.

| Database     | Purpose                                           |
| ------------ | ------------------------------------------------- |
| admin.db     | Users, sessions, API keys                         |
| content.db   | URLs, landing pages, preview pages, tags          |
| analytics.db | Visits, QR scans, statistics                      |
| system.db    | Audit events, jobs, migrations, health monitoring |

---

## Initial Setup

Create an administrator account:

### Native Installation

```bash
cargo run -- create-admin
```

### Docker

```bash
docker exec -it bzod bzod create-admin
```

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

### Server Status

![Server Status](screenshots/server-status.png)

---

## Docker Deployment

### Build

```bash
docker compose build
```

### Start

```bash
docker compose up -d
```

### Logs

```bash
docker logs -f bzod
```

---

## Docker Compose Example

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
      - ./config:/app/config

    environment:
      HOST: 0.0.0.0
      PORT: 8654
      DATA_DIR: /app/data
```

---

## Development

### Build

```bash
cargo build
```

### Run

```bash
cargo run -- serve
```

### Create Administrator

```bash
cargo run -- create-admin
```

### Run Tests

```bash
cargo test
```

---

## Project Structure

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
```

---

## Roadmap

Planned features:

* Vanity URLs
* CSV import/export
* Geo analytics
* Multi-user administration
* SSO integration
* Signed temporary links
* OpenAPI documentation
* Webhook support

---

## Production Deployment

Recommended stack:

```text
Internet
    │
    ▼
Nginx Proxy Manager
    │
    ▼
BZOD
    │
    ▼
SQLite
```

HTTPS is strongly recommended.

---

## License

Apache License 2.0

---

## Author

Sunil Purushottam Thakare

Built with Rust, SQLite, Axum, Askama, and a preference for simple, maintainable software.

