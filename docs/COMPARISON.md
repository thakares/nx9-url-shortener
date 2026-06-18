# BZOD vs Other Self-Hosted URL Shorteners

**BZOD (nx9-url-shortener)** is a modern, privacy-focused, self-hosted URL management platform built in Rust as part of the **NX9 Platform**.

Unlike many traditional URL shorteners that focus solely on redirects, BZOD combines:

* URL shortening
* Landing pages
* QR code generation
* QR analytics
* Password protection
* Link expiration
* REST API
* CLI automation
* Backup & restore
* Audit logging

into a single lightweight deployment.

**Philosophy:** *One binary. One command. Full ownership.*

---

## At a Glance

* Rust-based
* Single ~18 MB binary
* Embedded SQLite
* No external services required
* Landing pages
* QR generation & analytics
* Password-protected links
* REST API
* CLI automation
* Backup & restore
* Audit trail
* MIT OR Apache-2.0 licensed
* One-command deployment

---

## Quick Comparison

| Feature               | BZOD              | Shlink   | YOURLS   | Chhoto URL |
| --------------------- | ----------------- | -------- | -------- | ---------- |
| Language              | Rust              | PHP      | PHP      | Rust       |
| Single Binary         | ✅                 | ❌        | ❌        | ✅          |
| Landing Pages         | ✅                 | ❌        | Plugin   | ❌          |
| QR Code + Analytics   | ✅                 | Partial  | Plugin   | Partial    |
| Password Protection   | ✅                 | Limited  | Plugin   | ❌          |
| Backup & Restore      | ✅                 | External | External | ❌          |
| Audit Trail           | ✅                 | Limited  | Plugin   | ❌          |
| CLI Tools             | ✅                 | Limited  | Limited  | Limited    |
| Dependencies          | None              | PHP + DB | PHP + DB | None       |
| Deployment Complexity | Low               | Medium   | High     | Low        |
| License               | MIT OR Apache-2.0 | MIT      | MIT      | MIT        |

---

## Design Philosophy

| Principle                  | BZOD              |
| -------------------------- | ----------------- |
| Self-hosted                | ✅                 |
| Privacy-first              | ✅                 |
| Open Source                | MIT OR Apache-2.0 |
| Vendor Lock-in             | None              |
| Telemetry                  | None              |
| External Services Required | None              |
| Database Server Required   | No (SQLite)       |
| Runtime Dependencies       | None              |
| Single Binary              | Yes (~18 MB)      |
| Linux-first                | Yes               |

---

## The NX9 Philosophy

BZOD is part of the **NX9 Platform**.

NX9 projects follow strict engineering principles:

* Linux-native first
* Rust-first
* Single binary deployments
* No NodeJS
* No React
* No Python runtime dependencies
* No vendor lock-in
* No telemetry
* Privacy-first by default
* MIT OR Apache-2.0 licensed

GitHub and Codeberg are source-code repositories, not the projects themselves.

The software is the project.

The goal of NX9 is simple:

> Build technology that serves people, organizations, communities, and governments — not advertising networks, data brokers, or vendor ecosystems.

---

## Why BZOD Exists

Most self-hosted URL shorteners optimize for one of two extremes:

### 1. Minimal Redirect Service

A tiny application that creates short links and redirects traffic.

Advantages:

* Extremely lightweight
* Easy to understand
* Easy to maintain

Disadvantages:

* Limited administration
* Limited analytics
* Limited security features
* Often requires additional tools

### 2. Large Multi-Service Platform

Feature-rich systems with extensive integrations and dependencies.

Advantages:

* Powerful analytics
* Advanced routing
* Large ecosystems

Disadvantages:

* More infrastructure
* More maintenance
* Higher resource requirements

### BZOD's Approach

BZOD intentionally sits in the middle.

It is:

* Small enough for a Raspberry Pi
* Powerful enough for organizations
* Simple enough for homelabs
* Complete enough for production use

---

# BZOD vs Go URL Shorteners

Popular Go projects include:

* Krtk
* Slash
* Goshorly
* Shortr
* Custom Gin/Echo implementations

## Detailed Comparison

| Aspect              | BZOD (Rust)          | Typical Go Projects |
| ------------------- | -------------------- | ------------------- |
| Binary              | Single ~18 MB binary | Usually 10–20 MB    |
| Runtime             | None                 | None                |
| Database            | Embedded SQLite      | SQLite / PostgreSQL |
| Landing Pages       | ✅ Built-in           | Rare                |
| QR Generation       | ✅                    | Sometimes           |
| QR Analytics        | ✅                    | Rare                |
| Password Protection | ✅                    | Varies              |
| Link Expiry         | ✅                    | Often               |
| One-Time Links      | ✅                    | Rare                |
| UTM Builder         | ✅                    | Rare                |
| REST API            | ✅                    | Usually             |
| CLI                 | ✅ Extensive          | Usually limited     |
| Backup & Restore    | ✅ Built-in           | Rare                |
| Audit Logs          | ✅                    | Rare                |
| Admin Dashboard     | ✅                    | Varies              |

### Summary

Go shorteners are often:

* Extremely simple
* Fast
* Easy to extend

BZOD focuses on:

* Rich built-in functionality
* Complete ownership
* Minimal operations
* Batteries-included deployment

---

# BZOD vs Python URL Shorteners

Popular Python projects include:

* Pygmy
* ReducePy
* Schort
* Flask/FastAPI examples

## Detailed Comparison

| Aspect           | BZOD (Rust)           | Python Solutions  |
| ---------------- | --------------------- | ----------------- |
| Runtime          | None                  | Python required   |
| Deploy Size      | ~18 MB                | Often 100+ MB     |
| Memory Usage     | Very Low              | Moderate          |
| Landing Pages    | ✅                     | Rare              |
| QR Analytics     | ✅                     | Rare              |
| Backup & Restore | ✅                     | Rare              |
| CLI Tools        | ✅                     | Limited           |
| Dashboard        | ✅                     | Varies            |
| Security         | Argon2id + Audit Logs | Project dependent |
| Performance      | Excellent             | Good              |

### Summary

Python solutions are ideal when:

* Already using Python
* Rapid prototyping
* Easy customization

BZOD is ideal when:

* Long-term deployment matters
* Resource efficiency matters
* Minimal maintenance is desired

---

# BZOD vs Shlink

Shlink is one of the most mature self-hosted URL shorteners available.

## Detailed Comparison

| Aspect                   | BZOD          | Shlink           |
| ------------------------ | ------------- | ---------------- |
| Language                 | Rust          | PHP              |
| Deployment               | Single binary | PHP stack        |
| External DB              | No            | Usually yes      |
| Landing Pages            | ✅             | ❌                |
| Password Protected Links | ✅             | Limited          |
| QR Analytics             | ✅             | Partial          |
| UTM Builder              | ✅             | ❌                |
| Backup & Restore         | ✅             | External tooling |
| Audit Trail              | ✅             | Limited          |
| Dynamic Redirect Rules   | ❌             | ✅                |
| Multi-domain             | Planned       | ✅                |
| Ecosystem                | Growing       | Mature           |

### Summary

Choose Shlink when:

* Multi-domain management is critical
* Dynamic redirect rules are required
* Enterprise-scale analytics matter

Choose BZOD when:

* Simplicity matters
* Privacy matters
* Minimal infrastructure matters
* Landing pages are important

---

# BZOD vs YOURLS

YOURLS is the classic self-hosted URL shortener.

## Detailed Comparison

| Aspect        | BZOD          | YOURLS     |
| ------------- | ------------- | ---------- |
| Language      | Rust          | PHP        |
| Architecture  | Single binary | LAMP stack |
| Plugins       | Not required  | Extensive  |
| Landing Pages | ✅             | Plugin     |
| QR Analytics  | ✅             | Plugin     |
| Audit Logs    | ✅             | Plugin     |
| Backup Tools  | ✅             | External   |
| API           | ✅             | ✅          |
| Maintenance   | Minimal       | Moderate   |

### Summary

YOURLS wins on:

* Age
* Community
* Plugin ecosystem

BZOD wins on:

* Simplicity
* Deployment
* Modern architecture
* Integrated features

---

# BZOD vs Chhoto URL

Chhoto URL is the closest Rust-based competitor.

## Detailed Comparison

| Aspect           | BZOD   | Chhoto URL |
| ---------------- | ------ | ---------- |
| Language         | Rust   | Rust       |
| Landing Pages    | ✅      | ❌          |
| QR Codes         | ✅      | ✅          |
| QR Analytics     | ✅      | ❌          |
| Password Links   | ✅      | ❌          |
| Backup & Restore | ✅      | ❌          |
| REST API         | ✅      | JSON-RPC   |
| Audit Logs       | ✅      | ❌          |
| Analytics        | Rich   | Basic      |
| Binary Size      | ~18 MB | Smaller    |

### Summary

Choose Chhoto URL for:

* Maximum simplicity
* Minimal footprint

Choose BZOD for:

* Feature completeness
* Better administration
* Better analytics

---

## Future Comparisons

Additional comparison sections may be added in the future for:

* Bitly
* Dub
* Pygmy
* Krtk
* Other self-hosted URL management platforms

---

## Conclusion

**BZOD is not merely a URL shortener.**

It is a lightweight URL management platform that combines:

* Link shortening
* Landing pages
* QR services
* Analytics
* Automation
* Security
* Backups

into a single deployable Rust binary.

As part of the NX9 Platform, BZOD follows a simple principle:

> Own your links. Own your data. Own your infrastructure.

No telemetry. No vendor lock-in. No unnecessary complexity.

For users seeking privacy, simplicity, ownership, and long-term sustainability, BZOD offers a compelling alternative to both cloud SaaS platforms and traditional self-hosted URL shorteners.
