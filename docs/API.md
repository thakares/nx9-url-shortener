# BZOD REST API

> Programmatic access to URLs, Landing Pages, QR Codes, Analytics, and Audit Logs.

## Overview

The BZOD REST API allows automation and integration with external systems such as:

* Home Assistant
* Shell Scripts
* CI/CD Pipelines
* Monitoring Systems
* Internal Applications
* Self-hosted Services

All API endpoints require authentication using an API Token generated from:

```text
Admin Dashboard → Settings → REST API Tokens
```

---

# Authentication

Generate an API token from the Admin Dashboard.

Example token:

```text
bzo_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

Pass the token using the `Authorization` header.

## Example

```bash
curl \
  -H "Authorization: bzo_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx" \
  https://your-domain.com/api/v1/stats
```

---

# Base URL

```text
https://your-domain.com/api/v1
```

Example:

```text
https://bzo.in/api/v1
```

---

# Response Format

Successful responses:

```json
{
  "success": true,
  "data": {}
}
```

Error responses:

```json
{
  "success": false,
  "error": "Invalid API token"
}
```

---

# URL Management

## List URLs

```http
GET /api/v1/urls
```

### Example

```bash
curl \
  -H "Authorization: TOKEN" \
  https://your-domain.com/api/v1/urls
```

---

## Create URL

```http
POST /api/v1/urls
```

### Request

```json
{
  "code": "rust",
  "target_url": "https://www.rust-lang.org",
  "description": "Rust Language"
}
```

### Example

```bash
curl \
  -X POST \
  -H "Authorization: TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
        "code":"rust",
        "target_url":"https://www.rust-lang.org"
      }' \
  https://your-domain.com/api/v1/urls
```

---

## Get URL

```http
GET /api/v1/urls/{uuid}
```

Example:

```http
GET /api/v1/urls/5d4d9e98-7cb7-4c97-9a0a-123456789abc
```

---

## Update URL

```http
PUT /api/v1/urls/{uuid}
```

---

## Delete URL

```http
DELETE /api/v1/urls/{uuid}
```

---

## URL Preview

```http
GET /api/v1/urls/{uuid}/preview
```

Returns rendered metadata used by preview cards.

---

# Landing Pages

## List Pages

```http
GET /api/v1/pages
```

---

## Create Page

```http
POST /api/v1/pages
```

### Example Request

```json
{
  "title": "My Product",
  "slug": "product",
  "description": "Product Landing Page",
  "content": "<h1>Hello World</h1>"
}
```

---

## Get Page

```http
GET /api/v1/pages/{uuid}
```

---

## Update Page

```http
PUT /api/v1/pages/{uuid}
```

---

## Delete Page

```http
DELETE /api/v1/pages/{uuid}
```

---

# Analytics

## Global Statistics

```http
GET /api/v1/stats
```

Returns overall platform metrics.

Example response:

```json
{
  "total_urls": 125,
  "total_pages": 12,
  "total_clicks": 8431,
  "total_qr_scans": 241
}
```

---

## URL Statistics

```http
GET /api/v1/stats/url/{uuid}
```

Returns analytics for a single URL.

---

## Landing Page Statistics

```http
GET /api/v1/stats/page/{uuid}
```

Returns analytics for a single landing page.

---

# QR Codes

## Download QR Code

```http
GET /api/v1/qr/{code}
```

Example:

```http
GET /api/v1/qr/rust
```

Returns QR image.

---

# Bulk Operations

## Bulk QR Export

```http
POST /api/v1/bulk/qr
```

Generate QR codes for multiple URLs.

---

## Bulk URL Operations

```http
POST /api/v1/bulk/url
```

Bulk create, update, or manage URLs.

---

# Audit Log

## List Audit Events

```http
GET /api/v1/audit
```

Returns administrative activity history.

Example response:

```json
[
  {
    "event": "url_created",
    "user": "admin",
    "timestamp": "2026-06-17T14:30:00Z"
  }
]
```

---

# HTTP Status Codes

| Code | Description           |
| ---- | --------------------- |
| 200  | Success               |
| 201  | Created               |
| 400  | Invalid Request       |
| 401  | Authentication Failed |
| 403  | Access Denied         |
| 404  | Resource Not Found    |
| 409  | Conflict              |
| 500  | Internal Server Error |

---

# Security Notes

* API tokens are displayed only once during creation.
* Tokens are stored as hashes and cannot be recovered.
* Revoke unused tokens immediately.
* Always use HTTPS.
* Never embed API tokens in public repositories.

---

# Example: Create URL From Shell Script

```bash
TOKEN="bzo_xxxxxxxxxxxxxxxxx"

curl \
  -X POST \
  -H "Authorization: ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{
        "code":"example",
        "target_url":"https://example.com"
      }' \
  https://your-domain.com/api/v1/urls
```

---

# API Stability

The BZOD API follows semantic versioning.

Current API namespace:

```text
/api/v1
```

Future breaking changes will be introduced under a new versioned namespace.

Example:

```text
/api/v2
```
