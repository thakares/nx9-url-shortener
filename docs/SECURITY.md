# BZOD Security Guide

Version: v0.5.1

---

# Security Overview

BZOD is designed as a self-hosted URL shortener and landing page platform with a strong emphasis on:

* Multi-user isolation
* Secure authentication
* Role-based access control
* Auditability
* Data ownership
* Disaster recovery
* Operational simplicity

This document describes the security architecture, threat model, authentication mechanisms, authorization controls, and operational security recommendations for BZOD v0.5.0.

---

# Security Principles

BZOD follows several core principles:

1. Least Privilege
2. Tenant Isolation
3. Defense in Depth
4. Auditability
5. Secure Defaults
6. Explicit Ownership
7. Fail Secure

---

# Threat Model

BZOD is designed to protect against:

* Unauthorized dashboard access
* Credential theft
* Session hijacking
* Cross-user data access
* Slug takeover attempts
* Privilege escalation
* CSRF attacks
* XSS injection attempts
* Unauthorized API access
* Malicious content modification
* Accidental administrative mistakes

BZOD is not intended to defend against:

* Physical server compromise
* Root-level operating system compromise
* Malware running as the BZOD service user
* Full database theft by a privileged host administrator

---

# Authentication

Authentication is centralized in:

```text
users.db
```

Tables:

```text
users
sessions
api_tokens
```

All users authenticate through the same identity system.

---

# Password Security

Passwords are never stored in plaintext.

Stored values:

```text
password_hash
```

Passwords are hashed before storage.

Administrative password resets generate entirely new hashes.

Existing passwords cannot be recovered.

---

# Session Security

All dashboard authentication uses:

```text
bzod_session
```

cookie.

Sessions are stored in:

```text
users.db.sessions
```

Each session contains:

```text
session_id
user_id
created_at
expires_at
```

---

## Session Validation

Each authenticated request verifies:

1. Session exists
2. Session has not expired
3. User exists
4. User status is active
5. User has required permissions

Failure at any step immediately invalidates access.

---

## Session Revocation

Sessions are revoked when:

* User logs out
* User is disabled
* User is deleted
* Password is reset
* Administrator revokes sessions

---

## Session Fixation Protection

BZOD generates new session identifiers after successful authentication.

Previously issued identifiers are not reused.

---

# Authorization Model

BZOD implements Role-Based Access Control (RBAC).

Supported roles:

```text
admin
standard
system
```

---

## Administrator

Administrators can:

* Manage users
* Reset passwords
* Transfer ownership
* Manage quotas
* Access audit logs
* Review analytics
* Create backups
* Restore backups
* Moderate content

Administrators cannot bypass audit logging.

---

## Standard User

Standard users can:

* Manage owned URLs
* Manage owned landing pages
* View owned analytics
* Generate API tokens
* Manage owned content

Standard users cannot:

* Access other users' content
* Access administrative endpoints
* Access system settings

---

## System Accounts

System accounts are internal accounts.

They cannot authenticate into:

* Dashboard
* REST API

---

# Multi-User Isolation

Multi-user isolation is one of the primary security features of BZOD.

Each tenant receives independent databases.

Example:

```text
users/
├── 2/
│   ├── content.db
│   └── analytics.db
│
├── 3/
│   ├── content.db
│   └── analytics.db
```

User 2 never accesses:

```text
users/3/content.db
users/3/analytics.db
```

User 3 never accesses:

```text
users/2/content.db
users/2/analytics.db
```

---

# Global Slug Security

All public slugs are stored in:

```text
system.db.global_slugs
```

Each slug is globally unique.

Example:

```text
/company
```

may belong to only one owner.

Duplicate registrations are rejected.

---

## Slug Ownership

Every slug contains:

```text
owner_user_id
target_id
target_type
status
```

Ownership must match before modification is permitted.

---

## Slug Transfer Protection

Only administrators may transfer ownership.

Transfer operations:

1. Validate destination quotas
2. Validate destination user
3. Copy content
4. Update ownership
5. Record history
6. Write audit event

---

# API Security

REST API authentication uses API tokens.

Tokens are stored as hashes.

Plaintext tokens are shown only once during creation.

---

## API Token Security

Stored values:

```text
token_hash
```

Never:

```text
plaintext_token
```

If a token is lost:

1. Revoke it
2. Generate a new token

---

## API Permissions

Admin tokens:

```text
Full administrative access
```

Standard user tokens:

```text
Owned resources only
```

System accounts:

```text
API access denied
```

---

# CSRF Protection

All dashboard forms require valid CSRF tokens.

Protected actions include:

* Login
* User creation
* Password reset
* Content modification
* Moderation actions
* Quota updates
* Backup operations

---

## Invalid CSRF Requests

Invalid requests return:

```http
403 Forbidden
```

and are rejected before processing.

---

# XSS Protection

User-supplied content is validated before rendering.

Templates use:

```text
Askama
```

which escapes output by default.

Recommended:

* Do not allow arbitrary JavaScript
* Validate HTML content
* Restrict trusted editors

---

# Content Moderation

Administrators may:

* Flag content
* Disable content
* Delete content

Disabled content returns:

```http
410 Gone
```

for:

```text
/{slug}
/p/{slug}
/api/qr/{slug}.png
/api/qr/{slug}.svg
```

---

# Audit Logging

Security-sensitive actions are logged.

Examples:

```text
login
logout
failed_login
user_created
user_deleted
password_reset
slug_transfer
quota_update
backup_created
restore_executed
```

Stored in:

```text
system.db
```

Audit logs should be reviewed regularly.

---

# Backup Security

Backups may contain:

* User records
* Session records
* URLs
* Pages
* Analytics
* API token hashes

Backups should be treated as sensitive data.

---

## Recommendations

Store backups:

* Offsite
* Encrypted
* Access-controlled

Never expose backup archives publicly.

---

# Database Security

SQLite databases should be accessible only to the BZOD service account.

Recommended permissions:

```bash
chmod 700 data
chmod 600 *.db
```

---

# HTTPS Requirements

Production deployments should always use HTTPS.

Recommended reverse proxies:

* Nginx
* Caddy
* Traefik

Never expose login pages over plaintext HTTP.

---

# Security Headers

Recommended reverse proxy headers:

```http
X-Frame-Options: DENY
X-Content-Type-Options: nosniff
Referrer-Policy: strict-origin-when-cross-origin
Content-Security-Policy: default-src 'self'
```

---

# Password Policy Recommendations

Recommended minimum:

```text
12 characters
```

Encourage:

* Password managers
* Unique passwords
* Randomly generated credentials

Avoid:

* Reused passwords
* Dictionary words
* Predictable patterns

---

# Brute Force Protection

Recommended deployment protections:

* Reverse proxy rate limiting
* Fail2Ban
* Firewall rules

Example:

```text
5 login attempts
within 5 minutes
```

before temporary blocking.

---

# Administrative Security Checklist

Before production deployment:

* Enable HTTPS
* Configure backups
* Review file permissions
* Remove default credentials
* Verify audit logging
* Test restore procedures
* Review active sessions

---

# Incident Response

If compromise is suspected:

1. Disable affected accounts.
2. Revoke active sessions.
3. Revoke API tokens.
4. Create forensic backup.
5. Review audit logs.
6. Restore from trusted backups if necessary.
7. Rotate credentials.

---

# Security Testing

BZOD v0.5.0 includes tests covering:

* Authentication
* Authorization
* Session validation
* CSRF enforcement
* Slug ownership
* User isolation
* Upgrade migrations
* Backup integrity
* Disaster recovery

These tests are executed during CI and release validation.

---

# Responsible Disclosure

If a security vulnerability is discovered:

1. Do not publish exploit details immediately.
2. Report the issue privately.
3. Allow time for remediation.
4. Coordinate disclosure after a fix is available.

---

# Known Limitations

Current limitations include:

* No MFA support
* No SSO integration
* No hardware security key support
* No built-in rate limiter
* No WebAuthn support

These may be addressed in future releases.

---

# Summary

BZOD v0.5.0 provides:

* Centralized authentication
* Secure session management
* RBAC authorization
* Multi-user isolation
* Global slug ownership controls
* CSRF protection
* API token hashing
* Audit logging
* Backup security
* Operational security guidance

while maintaining a lightweight, SQLite-native, self-hosted architecture.

---

End of Document.
