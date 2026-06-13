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

## 9. Disaster Recovery Test

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

## 10. Database Health Verification

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

## 11. SQLite Integrity Checks

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

## 12. Web Interface Verification

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

## 13. Docker Verification

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

## 14. Upgrade Verification

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

# Guiding Principle

A successful release is not merely one that starts.

A successful release is one that can be recovered.
