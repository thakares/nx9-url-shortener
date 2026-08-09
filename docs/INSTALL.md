# BZOD Installation Guide

Version: v0.6.0

---

# Introduction

BZOD is a self-hosted multi-user URL management platform written in Rust.

Features include:

* URL shortening
* Landing pages
* QR code generation
* Analytics
* User management
* Audit logging
* Moderation
* Backup & restore
* Disaster recovery

BZOD is distributed as a single executable and uses SQLite databases for storage.

No PostgreSQL, MySQL, Redis, Elasticsearch, or external services are required.

---

# Installation Methods

BZOD supports three deployment methods:

| Method         | Recommended For  |
| -------------- | ---------------- |
| Docker Compose | Most deployments |
| Native Binary  | Linux servers    |
| Source Build   | Development      |

---

# System Requirements

## Minimum

| Component | Requirement  |
| --------- | ------------ |
| CPU       | 1 Core       |
| Memory    | 512 MB       |
| Storage   | 1 GB         |
| OS        | Linux x86_64 |

## Recommended

| Component | Requirement              |
| --------- | ------------------------ |
| CPU       | 2+ Cores                 |
| Memory    | 2 GB                     |
| Storage   | 10+ GB SSD               |
| OS        | Debian 12 / Ubuntu 24.04 |

## Tested Platforms

* Debian 12 Bookworm
* Ubuntu 22.04
* Ubuntu 24.04
* Arch Linux
* Docker
* CasaOS

---

# Installation Using Docker

## Prerequisites

Install:

```bash
docker
docker compose
```

Verify:

```bash
docker --version
docker compose version
```

---

## Create Directory

```bash
mkdir -p /opt/bzod
cd /opt/bzod
```

---

## Copy Files

Required:

```text
docker-compose.yml
Dockerfile
```

Optional:

```text
bzod.service
```

---

## Start Container

```bash
docker compose up -d
```

Verify:

```bash
docker compose ps
```

View logs:

```bash
docker compose logs -f
```

---

## Stop Container

```bash
docker compose down
```

---

## Restart Container

```bash
docker compose restart
```

---

# Native Installation

## Install Dependencies

### Debian / Ubuntu

```bash
sudo apt update

sudo apt install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    sqlite3
```

### Arch Linux

```bash
sudo pacman -S \
    base-devel \
    openssl \
    sqlite
```

---

## Download Release Binary

Example:

```bash
wget https://example.com/bzod-v0.6.0-linux-amd64.tar.gz
```

Extract:

```bash
tar -xzf bzod-v0.6.0-linux-amd64.tar.gz
```

Install:

```bash
sudo install -m755 bzod /usr/local/bin/bzod
```

Verify:

```bash
bzod --help
```

---

# Build From Source

## Install Rust

```bash
curl https://sh.rustup.rs -sSf | sh
```

Verify:

```bash
cargo --version
rustc --version
```

---

## Clone Repository

```bash
git clone https://github.com/thakares/nx9-url-shortener.git

cd nx9-url-shortener
```

---

## Build

Development:

```bash
cargo build
```

Release:

```bash
cargo build --release
```

Binary:

```bash
target/release/bzod
```

---

# Data Directory

BZOD automatically creates its databases on first startup.

Default structure:

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
    └── ...
```

Do not manually modify database files while BZOD is running.

---

# First Startup

Run:

```bash
bzod serve
```

By default:

```text
http://localhost:8080
```

Open:

```text
http://localhost:8080
```

---

# Bootstrap Administrator

On a fresh installation:

1. Open Login page
2. Use bootstrap credentials
3. Create the first administrator account
4. Save the credentials securely

After bootstrap:

* Bootstrap mode is disabled
* Normal authentication is enforced

---

# Create Administrator Using CLI

Alternative method:

```bash
bzod create-admin
```

Follow prompts:

```text
Username:
Password:
```

The administrator account is stored in:

```text
users.db
```

---

# Reverse Proxy Configuration

Using Nginx is recommended.

Example:

```nginx
server {
    server_name bzod.example.com;

    location / {
        proxy_pass http://127.0.0.1:8080;

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;

        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;

        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Reload:

```bash
sudo nginx -t
sudo systemctl reload nginx
```

---

# HTTPS

Recommended options:

* Let's Encrypt
* Nginx Proxy Manager
* Caddy
* Traefik

Always use HTTPS in production.

---

# Running as Systemd Service

Install binary:

```bash
sudo install -m755 bzod /usr/local/bin/bzod
```

Copy service:

```bash
sudo cp bzod.service /etc/systemd/system/
```

Reload:

```bash
sudo systemctl daemon-reload
```

Enable:

```bash
sudo systemctl enable bzod
```

Start:

```bash
sudo systemctl start bzod
```

Status:

```bash
sudo systemctl status bzod
```

Logs:

```bash
journalctl -u bzod -f
```

---

# Firewall

Open HTTP:

```bash
sudo ufw allow 8080/tcp
```

HTTPS:

```bash
sudo ufw allow 443/tcp
```

HTTP:

```bash
sudo ufw allow 80/tcp
```

---

# Health Verification

Open:

```text
http://localhost:8080
```

Login as administrator.

Verify:

* Dashboard loads
* User list loads
* URL creation works
* Landing pages work
* QR generation works
* Analytics record visits

---

# Upgrade Procedure

Always backup before upgrading.

Create backup:

```bash
bzod backup
```

Stop service:

```bash
sudo systemctl stop bzod
```

Replace binary.

Run migrations:

```bash
bzod migrate
```

Start service:

```bash
sudo systemctl start bzod
```

Verify logs.

See:

```text
docs/UPGRADE.md
```

---

# Troubleshooting

## Port Already In Use

Check:

```bash
ss -tulpn | grep 8080
```

Change port or stop conflicting service.

---

## Database Locked

Verify only one BZOD instance is running:

```bash
ps aux | grep bzod
```

---

## Permission Errors

Verify ownership:

```bash
chown -R bzod:bzod data/
```

---

## Login Problems

Verify:

* Administrator account exists
* Session cookies enabled
* System clock is correct

---

## View Logs

Systemd:

```bash
journalctl -u bzod -f
```

Docker:

```bash
docker compose logs -f
```

---

# Next Steps

After installation:

1. Read `MULTI_USER.md`
2. Read `ADMIN_GUIDE.md`
3. Configure backups
4. Configure HTTPS
5. Create additional users
6. Verify restore procedures

---

# Additional Documentation

| File              | Purpose                  |
| ----------------- | ------------------------ |
| ARCHITECTURE.md   | System architecture      |
| MULTI_USER.md     | Multi-user design        |
| ADMIN_GUIDE.md    | Administrative workflows |
| BACKUP_RESTORE.md | Backup procedures        |
| SECURITY.md       | Security model           |
| CLI.md            | Command reference        |
| API.md            | REST API reference       |
| UPGRADE.md        | Upgrade instructions     |

---

End of Document.
