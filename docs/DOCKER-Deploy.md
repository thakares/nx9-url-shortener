# Docker Deployment Guide for BZOD

This guide covers deployment, upgrades, backup, restore, troubleshooting, and production best practices for **BZOD (nx9-url-shortener)** using Docker.

---

# Overview

BZOD is a lightweight self-hosted URL shortener and landing page platform written in Rust.

Features include:

* URL shortening
* Human-readable custom slugs (`!office`, `!home`, etc.)
* Landing pages
* QR code generation
* Analytics
* Audit logging
* API access
* Backup and restore
* SQLite-based storage
* Docker deployment

BZOD is designed to remain simple:

* No PostgreSQL
* No Redis
* No external dependencies
* No vendor lock-in

---

# Quick Start

## Clone Repository

```bash
git clone https://github.com/thakares/nx9-url-shortener.git
cd nx9-url-shortener
```

## Build and Start

```bash
docker compose up -d --build
```

## Create Administrator

```bash
docker exec -it bzod bzod create-admin
```

Open:

```text
http://SERVER-IP:8654
```

Admin panel:

```text
http://SERVER-IP:8654/admin
```

---

# Docker Compose

Example:

```yaml
services:
  bzod:
    container_name: bzod
    build: .
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
      COOKIE_SECURE: "false"

    healthcheck:
      test: ["CMD", "./bzod", "doctor"]
      interval: 30s
      timeout: 10s
      retries: 3
```

Start:

```bash
docker compose up -d
```

Verify:

```bash
docker ps
docker logs -f bzod
```

---

# Directory Layout

Typical deployment:

```text
bzod/
├── docker-compose.yml
├── Dockerfile
├── config/
├── data/
│   ├── admin.db
│   ├── content.db
│   ├── analytics.db
│   └── system.db
└── backups/
```

---

# Root Landing Page

BZOD can serve a static landing page from:

```text
www/index.html
```

This page is available at:

```text
https://your-domain/
```

Examples:

```text
https://bzo.in/
https://short.example.com/
```

The root landing page is packaged automatically inside the Docker image.

---

# Reverse Proxy Configuration

BZOD is intended to run behind a reverse proxy.

Example Nginx configuration:

```nginx
server {
    server_name bzo.in;

    location / {
        proxy_pass http://127.0.0.1:8654;

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Example deployment:

```text
Internet
    ↓
Nginx Proxy Manager
    ↓
BZOD Docker Container
```

---

# Environment Variables

| Variable      | Description        | Default   |
| ------------- | ------------------ | --------- |
| HOST          | Bind address       | 0.0.0.0   |
| PORT          | Listen port        | 8654      |
| DATA_DIR      | Database directory | /app/data |
| COOKIE_SECURE | Secure cookies     | false     |
| RUST_LOG      | Logging level      | info      |

Production recommendation:

```text
COOKIE_SECURE=true
```

when HTTPS is enabled.

---

# Backup

## Web UI

Navigate to:

```text
Admin → Settings → Maintenance & DB Utilities
```

Click:

```text
Download Backup
```

A compressed archive containing all databases will be downloaded.

---

## CLI Backup

Create backup:

```bash
docker exec -it bzod bzod backup
```

Example output:

```text
backup-2026-06-14.tar.gz
```

---

# Restore

## Web UI Restore

Navigate to:

```text
Admin → Settings → Maintenance & DB Utilities
```

Upload:

```text
backup.tar.gz
```

Type:

```text
RESTORE
```

Confirm restore.

The system will:

1. Validate archive contents
2. Restore databases
3. Reinitialize database access
4. Redirect to login

---

## CLI Restore

Copy backup archive into container or mounted volume.

Run:

```bash
docker exec -it bzod bash

cd /app/data

bzod restore --file backup.tar.gz
```

---

# Disaster Recovery

Example recovery procedure:

```bash
docker compose down

# Restore backup archive

docker compose up -d
```

Verify:

```bash
docker exec -it bzod bzod doctor
docker exec -it bzod bzod validate
```

Check:

* URLs
* Landing pages
* Analytics
* Audit logs
* Settings

---

# Useful CLI Commands

Health:

```bash
docker exec -it bzod bzod doctor
```

Statistics:

```bash
docker exec -it bzod bzod stats
```

Validate databases:

```bash
docker exec -it bzod bzod validate
```

Create admin:

```bash
docker exec -it bzod bzod create-admin
```

Shorten URL:

```bash
docker exec -it bzod bzod shorten https://example.com
```

Custom slug:

```bash
docker exec -it bzod bzod shorten https://example.com --slug !office
```

Expand URL:

```bash
docker exec -it bzod bzod expand !office
```

---

# Upgrading

Pull latest source:

```bash
git pull
```

Rebuild:

```bash
docker compose build --no-cache
```

Restart:

```bash
docker compose up -d
```

Verify:

```bash
docker logs -f bzod
```

---

# Troubleshooting

## Read-Only SQLite Database

Symptoms:

```text
attempt to write a readonly database
```

Check ownership:

```bash
ls -lah data/
```

Fix permissions:

```bash
docker exec -u 0 -it bzod bash

chown -R bzod:bzod /app/data
```

Restart:

```bash
docker compose restart bzod
```

---

## Missing Root Landing Page

Symptoms:

```text
404 on /
```

Verify:

```bash
docker exec -it bzod ls -lah /app/www
```

Expected:

```text
/app/www/index.html
```

Rebuild image if necessary:

```bash
docker compose build --no-cache
docker compose up -d
```

---

## Health Check Failure

Inspect logs:

```bash
docker logs bzod
```

Run:

```bash
docker exec -it bzod bzod doctor
```

---

## Port Already In Use

Change host port mapping:

```yaml
ports:
  - "8080:8654"
```

Access:

```text
http://SERVER-IP:8080
```

---

# Production Recommendations

* Use HTTPS
* Run behind Nginx Proxy Manager or Nginx
* Use strong administrator credentials
* Schedule regular backups
* Periodically test restore procedures
* Monitor disk space
* Keep Docker images updated

---

# Validation Checklist

After deployment verify:

* [ ] Admin login works
* [ ] URL shortening works
* [ ] Custom slugs work
* [ ] Landing pages work
* [ ] QR generation works
* [ ] Analytics recorded
* [ ] Backup download works
* [ ] Restore workflow works
* [ ] Root landing page loads
* [ ] `bzod doctor` reports healthy

A deployment should not be considered production-ready until backup and restore procedures have been successfully tested.
