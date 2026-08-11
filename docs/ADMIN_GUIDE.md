# BZOD Administrator Guide

Version: v0.7.0

---

# Introduction

This guide is intended for BZOD administrators responsible for operating, maintaining, and managing a BZOD instance.

It covers:

* Administrator authentication
* User management
* Quotas
* Sessions
* Moderation
* Slug ownership
* Analytics
* Audit logs
* Backup and recovery
* Health monitoring
* Operational best practices

---

# Administrator Role

Administrators have full platform control.

Administrative capabilities include:

* Create users
* Modify users
* Disable users
* Delete users
* Reset passwords
* Manage quotas
* Review analytics
* Moderate content
* Transfer slug ownership
* Manage backups
* Review audit logs
* Monitor system health

Administrators cannot bypass audit logging.

All administrative actions are recorded.

---

# Login

Administrative login is available at:

```text
/login
```

Successful login redirects to:

```text
/admin
```

Authentication uses:

```text
users.db
```

Sessions are stored in:

```text
users.db.sessions
```

Cookie name:

```text
bzod_session
```

---

# Administrative Dashboard

Route:

```text
/admin
```

The dashboard provides a high-level overview of platform activity.

Metrics include:

* Total Users
* Active Users
* Total URLs
* Total Landing Pages
* Active Sessions
* API Tokens
* Storage Usage
* Moderation Events
* Recent Audit Events

Quick actions include:

* Create User
* View Sessions
* View Audit Logs
* Create Backup
* Review Health Status

---

# User Management

## Users List

Route:

```text
/admin/users
```

Displays:

* User ID
* Username
* Status
* Account Type
* Creation Date

Available actions:

* View
* Edit
* Disable
* Enable
* Reset Password
* Delete

---

## Create User

Route:

```text
/admin/users/new
```

Fields:

* Username
* Password
* Account Type
* Quota Limits

Supported account types:

```text
admin
standard
```

Reserved usernames cannot be used.

Examples:

```text
admin
legacy_admin
system
root
administrator
```

---

## User Detail Page

Route:

```text
/admin/users/{id}
```

Displays:

### Profile

* User ID
* Username
* Status
* Account Type
* Created Date

### Usage Statistics

* URL Count
* Landing Page Count
* Visit Count
* Storage Usage
* API Token Count
* Active Sessions

### Quotas

* Maximum URLs
* Maximum Pages
* Maximum Storage
* Maximum Tokens

### Sessions

List of active sessions.

### API Tokens

List of active tokens.

---

## Edit User

Route:

```text
/admin/users/{id}/edit
```

Administrators may:

* Change status
* Change account type
* Modify quotas

---

## Reset Password

Route:

```text
/admin/users/{id}/password
```

Creates a new password hash and invalidates existing sessions.

Audit event generated:

```text
password_reset
```

---

## Disable User

Route:

```text
/admin/users/{id}/disable
```

Effects:

* User login disabled
* Existing sessions revoked
* API access denied

Audit event generated:

```text
user_disabled
```

---

## Enable User

Route:

```text
/admin/users/{id}/enable
```

Restores account access.

Audit event generated:

```text
user_enabled
```

---

## Delete User

Route:

```text
/admin/users/{id}/delete
```

Deletion performs:

1. Session revocation
2. API token removal
3. Content removal
4. Analytics removal
5. Slug release
6. User database deletion

Audit event generated:

```text
user_deleted
```

---

# Session Management

Route:

```text
/admin/sessions
```

Displays all active platform sessions.

Information displayed:

* User ID
* Username
* Session Identifier
* Created Time
* Expiry Time
* IP Address
* User Agent

---

## Revoke Session

Individual sessions can be revoked.

Effects:

* Session removed immediately
* User forced to reauthenticate

---

## Revoke All Sessions

Administrators may invalidate all active sessions.

Useful after:

* Password compromise
* Security incidents
* Large configuration changes

---

# Quota Management

Route:

```text
/admin/quotas
```

Quotas limit user resource consumption.

Available limits:

```text
max_urls
max_pages
max_storage_mb
max_api_tokens
```

---

## Quota Reconciliation

Administrators can execute:

```text
quota_reconcile
```

Purpose:

* Detect counter drift
* Recount resources
* Repair quota usage

Common causes:

* Manual database modifications
* Failed migrations
* Interrupted operations

---

# Moderation

Route:

```text
/admin/moderation
```

Moderation allows administrators to manage abuse and policy violations.

---

## Flag Content

Marks content for review.

Audit event:

```text
content_flagged
```

---

## Disable Content

Disabled content returns:

```http
410 Gone
```

Affected endpoints:

```text
/{slug}
/p/{slug}
/api/qr/{slug}.png
/api/qr/{slug}.svg
```

Audit event:

```text
content_disabled
```

---

## Enable Content

Restores functionality.

Audit event:

```text
content_enabled
```

---

## Delete Content

Permanently removes content.

Audit event:

```text
content_deleted
```

---

# Slug Management

Route:

```text
/admin/slugs
```

Displays platform-wide slug ownership.

Information includes:

* Slug
* Owner
* Type
* Status
* Creation Date

---

## Slug Types

Supported types:

```text
url
page
```

---

## Transfer Ownership

Administrators may transfer ownership.

Workflow:

1. Validate recipient quota.
2. Copy content.
3. Update ownership.
4. Update global slug registry.
5. Write audit record.

Audit event:

```text
slug_transfer
```

Analytics are preserved.

---

# Analytics

Administrators can access analytics for any managed resource.

---

## URL Analytics

Route:

```text
/admin/analytics/url/{id}
```

Displays:

* Total Visits
* Unique Visitors
* Referrers
* Browsers
* Countries
* Visit Timeline

---

## Page Analytics

Route:

```text
/admin/analytics/page/{id}
```

Displays identical metrics for landing pages.

---

## User Analytics

Administrators can review user-level analytics.

Route:

```text
/analytics
```

Includes:

* Top Links
* Top Pages
* Referrers
* Browsers
* Countries
* Recent Visits

---

# Audit Logs

Route:

```text
/admin/audit
```

All administrative actions are recorded.

Searchable event types include:

```text
login
logout
failed_login
user_created
user_deleted
user_disabled
user_enabled
password_reset
quota_updated
slug_transfer
content_flagged
content_disabled
backup_created
restore_executed
```

Audit logs should be reviewed regularly.

---

# Backup Management

Route:

```text
/admin/backups
```

Provides web-based backup operations.

---

## Create Backup

Creates a platform snapshot.

Includes:

```text
users.db
system.db
tenant databases
```

Audit event:

```text
backup_created
```

---

## Download Backup

Allows local storage of backup archives.

Recommended frequency:

```text
Daily
```

---

## Restore Backup

Restores a selected backup archive.

Audit event:

```text
restore_executed
```

Always test restores before production use.

---

## Delete Backup

Removes backup archives from storage.

---

# Health Dashboard

Route:

```text
/admin/health
```

Provides operational diagnostics.

Displays:

* Database Status
* WAL Status
* Storage Utilization
* Backup Status
* Health Check Results
* Quota Reconciliation Results

---

## Database Health

Checks:

```text
users.db
system.db
content.db
analytics.db
```

Reports:

```text
healthy
warning
error
```

---

## Storage Monitoring

Shows:

* Total Storage
* Free Storage
* Database Sizes
* Backup Sizes

---

# Security Administration

## Password Policies

Recommendations:

* Minimum 12 characters
* Unique passwords
* Password manager usage

---

## Session Management

Recommended actions:

* Revoke old sessions
* Review active sessions
* Remove inactive users

---

## CSRF Protection

All administrative forms require valid CSRF tokens.

Invalid requests return:

```http
403 Forbidden
```

---

## Audit Reviews

Recommended review schedule:

| Event Type        | Frequency |
| ----------------- | --------- |
| Failed Logins     | Daily     |
| User Creation     | Weekly    |
| Slug Transfers    | Weekly    |
| Backup Events     | Daily     |
| Moderation Events | Weekly    |

---

# Disaster Recovery

Recommended workflow:

1. Stop BZOD.
2. Create backup copy.
3. Restore archive.
4. Verify databases.
5. Run integrity checks.
6. Restart service.

---

# Operational Best Practices

Recommended:

* Enable HTTPS
* Run daily backups
* Monitor disk usage
* Review audit logs
* Keep binaries updated
* Test restore procedures regularly

Avoid:

* Manual database modifications
* Direct deletion of tenant databases
* Disabling audit logging

---

# Troubleshooting

## User Cannot Login

Check:

* User status
* Session validity
* Password reset history

---

## Slug Already Exists

Check:

```text
/admin/slugs
```

for ownership conflicts.

---

## Analytics Missing

Verify:

* Analytics worker running
* Analytics database present
* Event queue processing

---

## Backup Failure

Check:

* Free disk space
* File permissions
* Backup destination path

---

# Summary

The BZOD administration system provides:

* Centralized user management
* Quotas and session controls
* Moderation and slug ownership management
* Analytics visibility
* Audit logging
* Backup and restore capabilities
* Health monitoring

while maintaining strong tenant isolation and a SQLite-native operational model.

---

End of Document.
