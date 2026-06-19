# BZOD v0.5.0 — General Availability

**Release Date:** 2026-06-19

**BZOD v0.5.0** is the largest release in project history and marks the transition from a single-user URL shortener into a **production-ready multi-user platform** with tenant isolation, administration tools, analytics, moderation, backups, auditing, and comprehensive validation coverage.

## Key Highlights

- **Multi-User Architecture** with strong tenant isolation
- **Global Slug Namespace** with ownership tracking
- **New Administration Portal** for user management and moderation
- **User Self-Service Portal** with dedicated dashboards
- **Enhanced Analytics** with per-user and platform-wide views
- **Comprehensive Backup & Recovery** tooling
- **Upgrade Framework** with automated migration validation
- **90+ Automated Tests** covering multi-user scenarios, upgrades, and disaster recovery

---

## Multi-User Architecture

BZOD now supports multiple isolated users on a single deployment.

**Key capabilities:**
- Per-user content and analytics databases
- User quotas and status management
- User sessions and API tokens
- Strong tenant isolation enforcement

Each user's data is strictly separated.

## Administration Portal

New administrative dashboards include:

**User Management**
- Create, delete, enable, disable users
- Password resets and quota management

**Moderation Tools**
- Flag, disable, or re-enable content
- Moderation history

**Session & Audit Management**
- View and revoke sessions
- Full audit viewer

**Backup Management**
- Create, restore, and download backups

## User Self-Service Portal

Standard users now have dedicated dashboards for:
- Profile & password management
- Link and landing page management
- QR code generation
- Personal analytics
- API token management

## Security & Reliability

- Improved authentication and authorization
- Enhanced CSRF, SQL injection, and path traversal protection
- SQLite WAL mode with better transaction handling
- Comprehensive backup & recovery with rollback protection

## Testing & Validation

**Passed:**
- 90+ automated tests
- Full multi-user validation
- Upgrade path verification
- Backup/Restore + Disaster recovery tests
- Security regression tests
- Concurrency and WAL recovery tests

## Breaking Changes

The internal storage model has changed significantly to support multi-user operation.

**Administrators upgrading from v0.4.x should review:**
- `UPGRADE.md`
- `MULTI_USER.md`
- `BACKUP_RESTORE.md`

---

## Get Started

**One-command installation:**

```bash
curl -fsSL https://bzo.in/deploy.sh | sudo bash