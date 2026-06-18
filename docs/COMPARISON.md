# BZOD vs Other Self-Hosted URL Shorteners

**BZOD (nx9-url-shortener)** is a modern, privacy-focused, self-hosted URL management platform built in Rust as part of the **NX9 Platform**.

Unlike many traditional URL shorteners that focus only on redirects, BZOD combines:

* URL shortening
* Landing pages
* QR code generation
* Analytics
* Password protection
* Backup & restore
* REST API
* CLI automation
* Audit logging

into a single lightweight deployment.

The project philosophy is:

> One binary. One command. Full ownership.

---

# Design Philosophy

| Principle                  | BZOD              |
| -------------------------- | ----------------- |
| Self-hosted                | ✅                 |
| Privacy-first              | ✅                 |
| Open Source                | MIT OR Apache-2.0 |
| Vendor Lock-in             | None              |
| Telemetry                  | None              |
| External Services Required | None              |
| Database Server Required   | No                |
| Runtime Dependencies       | No                |
| Single Binary              | Yes               |
| Linux-first                | Yes               |

---

# BZOD vs Go URL Shorteners

Popular Go projects include:

* Krtk
* Slash
* Goshorly
* Shortr
* Custom Gin/Echo implementations

## Comparison

| Aspect              | BZOD (Rust)          | Typical Go Projects |
| ------------------- | -------------------- | ------------------- |
| Binary              | Single ~18 MB binary | Usually 10–20 MB    |
| Runtime             | None                 | None                |
| Database            | Embedded SQLite      | SQLite / PostgreSQL |
| Landing Pages       | ✅ Built-in           | Rare                |
| QR Generation       | ✅ PNG + SVG          | Sometimes           |
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

## Summary

Go shorteners are often:

* Extremely simple
* Fast
* Easy to extend

BZOD focuses on:

* Rich features
* Complete ownership
* Minimal operations
* Batteries included

---

# BZOD vs Python URL Shorteners

Popular Python projects include:

* Pygmy
* ReducePy
* Schort
* Flask/FastAPI examples

## Comparison

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

## Summary

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

Shlink is one of the most mature self-hosted URL shorteners.

## Comparison

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

## Summary

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

## Comparison

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

## Summary

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

## Comparison

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

## Summary

Choose Chhoto URL for:

* Maximum simplicity
* Minimal footprint

Choose BZOD for:

* Feature completeness
* Better administration
* Better analytics

---

# Why BZOD Exists

Most self-hosted URL shorteners optimize for one of two extremes:

1. Extremely simple redirect service
2. Large multi-service platform

BZOD attempts to sit in the middle:

* Small enough for a Raspberry Pi
* Powerful enough for organizations
* Simple enough for homelabs
* Complete enough for production use

---

# The NX9 Philosophy

BZOD is part of the **NX9 Platform**.

NX9 projects follow strict engineering principles:

* Linux-native first
* Rust-first
* Single binary deployments
* No NodeJS
* No React dependency
* No Python runtime dependency
* No vendor lock-in
* No telemetry
* Privacy-first
* Self-hostable
* MIT OR Apache-2.0 licensed

GitHub and Codeberg repositories are only source code hosting locations.

The actual project is the software itself.

The goal of NX9 is simple:

> Build technology that serves people, organizations, communities, and governments — not advertising networks, data brokers, or vendor ecosystems.

---

# Conclusion

BZOD is not merely a URL shortener.

It is a lightweight URL management platform that combines:

* Link shortening
* Landing pages
* QR ecosystem
* Analytics
* Automation
* Security
* Backups

into a single deployable Rust binary.

For users seeking privacy, simplicity, ownership, and long-term sustainability, BZOD offers a compelling alternative to both cloud SaaS platforms and traditional self-hosted URL shorteners.
