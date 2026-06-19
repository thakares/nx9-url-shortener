# TESTING.md

# BZOD Test Procedures

This document describes the official verification procedures for BZOD.

The objective is not merely to confirm that code compiles, but to ensure that the complete platform can be built, deployed, backed up, restored, migrated, and recovered successfully.

---

# Philosophy

BZOD prioritizes:

1. Data Integrity
2. Operational Simplicity
3. Recovery Capability
4. Deployment Reproducibility
5. Functional Correctness

A passing unit test suite alone is insufficient.

A release is considered valid only if backup, restore, migration, and recovery procedures have been verified.

---

# Test Categories

## 1. Build Verification

Verify the application compiles successfully.

```bash
cargo check
cargo build
cargo build --release
```

Expected Result:

* No compiler errors
* No panics during startup
* Release binary generated successfully

---

## 2. Static Analysis

```bash
cargo fmt --check
cargo clippy --all-targets
```

Expected Result:

* Formatting passes
* No significant Clippy warnings

---

## 3. Unit Tests

```bash
cargo test
```

Expected Result:

* All tests pass
* No ignored critical tests

---

## 4. Database Initialization

Create a clean environment.

```bash
rm -rf data

./bzod stats
```

Expected Result:

* Databases are automatically created
* Migrations applied successfully

Verify:

```bash
./bzod doctor
```

Expected Result:

```text
Overall status: HEALTHY
```

---

## 5. Migration Verification

Run migrations repeatedly.

```bash
./bzod migrate
./bzod migrate
./bzod migrate
```

Expected Result:

* No duplicate migrations
* No errors
* Schema remains stable

---

## 6. Administrator Creation

Create an administrator account.

```bash
./bzod create-admin
```

Expected Result:

* User created successfully
* Authentication works

Attempt duplicate creation:

```bash
./bzod create-admin
```

Expected Result:

* Duplicate username rejected

---

## 7. Backup Verification

Create backup archive.

```bash
./bzod backup
```

Expected Result:

* Backup archive generated
* Archive contains all databases

Verify:

```bash
tar -tzf backup-*.tar.gz
```

Expected Result:

```text
admin.db
content.db
analytics.db
system.db
```

---

## 8. Restore Verification

Create sample data.

Generate:

* Administrator
* URL records
* Landing pages
* Analytics records

Create backup:

```bash
./bzod backup
```

Delete databases:

```bash
rm -rf data
```

Restore:

```bash
./bzod restore --file backup.tar.gz
```

Expected Result:

* Restore completes successfully
* All records preserved

Verify:

```bash
./bzod doctor
./bzod stats
```

Expected Result:

```text
Overall status: HEALTHY
```

and original record counts preserved.

---
## 9. Disaster Recovery Scenario

1. Create backup
2. Stop container
3. Delete databases
4. Restore from backup
5. Fix permissions
6. Restart container
7. Validate:
    - URLs
    - Landing pages
    - Audit logs
    - Settings
    - Analytics
    - Status page

Expected Result:
System fully restored without data loss.
## 10. Disaster Recovery Test

This is the most important test.

Procedure:

1. Backup system.
2. Delete entire data directory.
3. Restore backup.
4. Start server.
5. Login to Admin UI.

Commands:

```bash
./bzod backup

rm -rf data

./bzod restore --file backup.tar.gz

./bzod serve
```

Expected Result:

* System fully operational
* No manual database repair required

---

## 11. Database Health Verification

Run:

```bash
./bzod doctor
```

Expected Result:

For every database:

```text
Integrity: ok
Foreign keys: enabled
Journal mode: wal
```

Final result:

```text
Overall status: HEALTHY
```

---

## 12. SQLite Integrity Checks

Manual verification.

```bash
sqlite3 data/admin.db "PRAGMA integrity_check;"
sqlite3 data/content.db "PRAGMA integrity_check;"
sqlite3 data/analytics.db "PRAGMA integrity_check;"
sqlite3 data/system.db "PRAGMA integrity_check;"
```

Expected Result:

```text
ok
```

for all databases.

---

## 13. Web Interface Verification

Start server.

```bash
./bzod serve
```

Verify:

* Homepage loads
* Redirects function
* Landing pages render
* Admin login works
* Dashboard loads
* API endpoints respond

---

## 14. Docker Verification

Build image.

```bash
docker compose build --no-cache
```

Start service.

```bash
docker compose up -d
```

Verify:

```bash
docker compose logs -f
```

Expected Result:

```text
Listening for requests
```

Verify:

```bash
./bzod doctor
```

inside container.

---

## 15. Upgrade Verification

1. Create backup.
2. Upgrade binary.
3. Run migration.
4. Start service.

```bash
./bzod backup

./bzod migrate

./bzod serve
```

Expected Result:

* Existing data preserved
* No migration failures

---
## Analytics Verification

Verify:

* URL analytics page loads
* Landing page analytics page loads
* Visitor activity table renders
* Empty visitor tables render correctly
* CSV export downloads successfully
* JSON export downloads successfully
* Date filtering works
* Invalid date filters return HTTP 400
* Pagination preserves active filters
* Exports respect active filters
# Release Acceptance Criteria

A release is considered production-ready only if:

* Build verification passes
* Static analysis passes
* Unit tests pass
* Backup verification passes
* Restore verification passes
* Disaster recovery verification passes
* Doctor reports HEALTHY
* Docker deployment succeeds
* Web UI functions correctly

Failure of backup, restore, or disaster recovery tests is considered a release blocker.

---

# BZOD v0.5.0

## Overview

BZOD v0.5.0 includes a comprehensive automated validation suite covering functionality, security, migrations, disaster recovery, concurrency, multi-user isolation, and operational workflows.

The goal is to ensure production upgrades and deployments can be performed safely with minimal risk.

---

# Test Categories

## Core Unit Tests

Validates:

* SQLite configuration
* WAL configuration
* Integrity checks
* Analytics helpers
* QR code generation
* Database initialization

---

## Authentication Tests

Validates:

* Session creation
* Session expiration
* API token creation
* API token revocation

Files:

* auth_tests.rs
* auth_migration_tests.rs

---

## User Management Tests

Validates:

* User creation
* Password reset
* Disable / Enable
* Status transitions
* Reserved usernames

Files:

* user_management_tests.rs

---

## Multi-User Isolation Tests

Validates:

* Database isolation
* Cross-user access denial
* Tenant separation

Files:

* user_isolation_tests.rs

---

## Slug Namespace Tests

Validates:

* Global uniqueness
* Reserved slugs
* Slug release behavior
* Slug ownership transfers

Files:

* slug_namespace_tests.rs
* slug_transfer_tests.rs

---

## Security Tests

Validates:

* CSRF protections
* Session expiration
* Bootstrap credential invalidation
* SQL injection resistance
* Path traversal rejection

Files:

* security_tests.rs

---

## Backup & Disaster Recovery Tests

Validates:

* Backup generation
* Restore operations
* Backup metadata integrity
* Corrupted backup rejection
* Rollback on restore failures

Files:

* backup_restore_tests.rs
* disaster_recovery_tests.rs

---

## Upgrade Validation Tests

Validates migration from legacy v0.4.0 deployments.

Checks:

* Database relocation
* Analytics preservation
* Link preservation
* Credential compatibility
* Redirection integrity

Files:

* upgrade_validation_tests.rs

---

## HTTP End-to-End Tests

Validates:

* Login
* Logout
* Session cookies
* CSRF protection
* Administrative authorization

Files:

* http_e2e_tests.rs

---

## Business Workflow Tests

Scenario A

Administrator creates user → User logs in → User creates link → Visitor accesses link → Analytics recorded.

Scenario B

Administrator disables user → Sessions invalidated → Login rejected.

Scenario C

Slug transfer between users → Redirect preserved → Analytics preserved.

Files:

* business_workflow_tests.rs

---

## Concurrency Tests

Validates:

* Concurrent slug creation
* Namespace consistency

Files:

* concurrency_tests.rs

---

## WAL Recovery Tests

Validates:

* SQLite WAL durability
* Recovery after backup operations

Files:

* wal_recovery_tests.rs

---

# Running All Tests

```bash
cargo test --all-targets -- --nocapture
```

# Release Validation

Before every release:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets -- --nocapture
cargo build --release
cargo audit
```

A release is considered valid only if all steps complete successfully.


# Guiding Principle

A successful release is not merely one that starts.

A successful release is one that can be recovered.
