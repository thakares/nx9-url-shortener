# BZOD Command Line Interface (CLI)

BZOD includes a comprehensive command-line interface for server administration, backups, migrations, diagnostics, validation, and multi-user management.

The current command list for BZOD v0.6.0 is:

```text
$ bzod --help

BZOD - Personal Redirector & Landing Page Platform

Usage: bzod <COMMAND>

Commands:
  serve           Start the BZOD web server
  backup          Create a tar.gz backup of all databases
  restore         Restore databases from a tar.gz backup file
  migrate         Apply pending database schema migrations
  stats           Print database statistics and record counts in the terminal
  validate        Perform a one-shot validation of all registered short link destinations
  create-admin    Create a new administrator user in the database
  doctor          Run database diagnostics and health checks
  shorten         Shorten a URL (Feature 3)
  expand          Expand a shortened code or custom slug to its destination URL (Feature 4)
  create-user     Create a new standard user in the database
  delete-user     Delete a standard user and all their databases/slugs
  disable-user    Disable a standard user
  enable-user     Enable a standard user
  reset-password  Reset standard user's password
  list-users      List all standard/system users
  backup-user     Backup a standard user's databases to a .tar.zst package
  restore-user    Restore a standard user's databases from a .tar.zst package
  admin-migrate   FUTURE: Migrate legacy admin content to a specific admin tenant database
  repair          Repair registry and database inconsistencies
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

---

# Server Operations

## Start Web Server

```bash
bzod serve
```

---

# Backup & Recovery

## Full Backup

```bash
bzod backup
```

Creates a compressed backup archive containing:

* users.db
* system.db
* content databases
* analytics databases
* user directories

## Full Restore

```bash
bzod restore backup.tar.gz
```

Restores an entire BZOD installation from a backup archive.

---

# Database Operations

## Apply Migrations

```bash
bzod migrate
```

Applies any pending database migrations.

Safe to execute multiple times.

## Database Statistics

```bash
bzod stats
```

Displays database statistics, record counts, storage usage, and operational metrics.

---

# Validation & Diagnostics

## Validate Links

```bash
bzod validate
```

Checks all registered URLs and reports invalid destinations.

## Health Diagnostics

```bash
bzod doctor
```

Performs:

* SQLite integrity checks
* WAL validation
* Database availability checks
* Storage verification
* System health diagnostics
* Global registry integrity validation

## Registry Repair

```bash
bzod repair registry --dry-run
```

Provides a transaction-safe repair utility for fixing global slug registry inconsistencies detected by `bzod doctor`.

* Use `--dry-run` to preview changes safely.
* Use `--force` to execute changes and remove orphaned entries.
* Use `--slug <slug>` to target a single missing entry.

---

# URL Management

## Create Short URL

```bash
bzod shorten https://example.com
```

## Expand Existing URL

```bash
bzod expand abc123
```

Returns the destination URL associated with the slug.

---

# Administrator Management

## Create Administrator

```bash
bzod create-admin admin
```

Creates a new administrator account.

---

# User Management

## List Users

```bash
bzod list-users
```

Displays all users in the platform.

## Create User

```bash
bzod create-user alice
```

Creates a new standard user.

## Disable User

```bash
bzod disable-user alice
```

Blocks login and invalidates sessions.

## Enable User

```bash
bzod enable-user alice
```

Re-enables a disabled user.

## Reset Password

```bash
bzod reset-password alice
```

Resets a user's password.

## Delete User

```bash
bzod delete-user alice
```

Deletes:

* User account
* User databases
* Sessions
* API tokens
* Slug ownership

---

# User Backup Operations

## Backup User

```bash
bzod backup-user alice
```

Creates a portable `.tar.zst` archive containing all user-owned data.

## Restore User

```bash
bzod restore-user alice.tar.zst
```

Restores a user from a previously generated archive.

---

# Recommended Maintenance

Daily:

```bash
bzod doctor
```

Weekly:

```bash
bzod backup
```

Before Upgrades:

```bash
bzod backup
bzod validate
```

After Upgrades:

```bash
bzod migrate
bzod doctor
```

---

# Related Documentation

* INSTALL.md
* MULTI_USER.md
* ADMIN_GUIDE.md
* BACKUP_RESTORE.md
* SECURITY.md
* API.md
* ARCHITECTURE.md
