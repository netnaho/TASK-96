# TalentFlow API

**Type: backend**

Offer, Onboarding, and Booking Platform — Rust backend built with Actix-web, Diesel, and PostgreSQL.

## Status

**Phase: Complete**

| Feature | Status |
|---------|--------|
| Repository scaffold and schema | Complete |
| Authentication (Argon2id, lockout, CAPTCHA, rate limiting) | Complete |
| Session persistence (PostgreSQL, idle timeout) | Complete |
| RBAC middleware (fail-closed) with scope helpers | Complete |
| Candidate CRUD with PII encryption (AES-256-GCM) | Complete |
| Object-level authorization (platform_admin / club_admin / member) | Complete |
| Offer CRUD with state machine + approval chain | Complete |
| CompensationData encryption + validation | Complete |
| Onboarding checklist + readiness calculation | Complete |
| Booking hold with 15-min auto-release (real scheduler) | Complete |
| Inventory slot reservation (SELECT FOR UPDATE) | Complete |
| Overbooking prevention (DB-level constraints + row locks) | Complete |
| 5-check eligibility gate before confirmation | Complete |
| Agreement evidence (typed name + timestamp + SHA-256 hash) | Complete |
| 24-hour breach rules for cancel/reschedule | Complete |
| Idempotency-Key deduplication on all mutating writes | Complete |
| Booking state machine (6 states, explicit transitions) | Complete |
| **Search/discovery (keyword + tag + status, deterministic scoring)** | Complete |
| **Autocomplete (vocabulary + historical queries, deduped)** | Complete |
| **Spell-correction from vocabulary + query history (edit distance ≤ 2)** | Complete |
| **Reporting subscriptions (offers_expiring, breach_rate, snapshot)** | Complete |
| **Daily snapshot job (DST-aware wall-clock, threshold alerts)** | Complete |
| **Dashboard version history (immutable versions, newest-first)** | Complete |
| **Integration connectors (encrypted auth config, inbound/outbound/bidirectional)** | Complete |
| **Incremental sync with last_updated_at watermarks** | Complete |
| **File-based import/export fallback (NDJSON, offline-ready)** | Complete |
| **User CRUD + role/permission management (admin-only)** | Complete |
| **Audit event read API (admin-only, append-only)** | Complete |
| **Office sites read API (seeded, read-only)** | Complete |
| **Authorization hardened (permission + object-level on all endpoints)** | Complete |
| **Input validation on all request DTOs (per_page capped at 100)** | Complete |
| Structured local logging (tracing, JSON output, no secrets) | Complete |
| Audit events for all mutations | Complete |
| Unit tests (state machine, agreement hash, 24h rules, eligibility, search scoring) | Complete |
| API integration tests (hold, overbooking, auth, cancel, idempotency, search, reporting, integrations) | Complete |
| Full documentation (booking lifecycle, search scoring, hold rules, breach, eligibility) | Complete |

## Quick Start

```bash
# Local development (with seeded users and known dev passwords):
docker-compose -f docker-compose.yml -f docker-compose.dev.yml up

# Production / shared environments (no seeding, migrations only):
docker-compose up
```

> **Note:** `./run_tests.sh --docker` is the canonical test command for this project. No local Rust toolchain required.

The dev override command:
1. Starts PostgreSQL 16 on port **5433** (host) → 5432 (container)
2. Builds the Rust application
3. Runs Diesel migrations automatically
4. Seeds default roles, users, permissions, vocabularies, and sites
5. Starts the API server on port 8080
6. Enables the background scheduler (hold auto-release, session cleanup, daily reporting snapshot)

The base `docker-compose up` does **not** seed — it only runs migrations and
starts the server.  See [seeds/README.md](seeds/README.md) for seeding details.

No host dependencies required beyond Docker and Docker Compose.

## Default Seed Users (dev override only)

Seed passwords are set via environment variables.  The `docker-compose.dev.yml`
override provides these known values for local development:

| Username | Role | Env var | Dev password |
|---|---|---|---|
| `platform_admin` | platform_admin | `SEED_ADMIN_PASSWORD` | `Admin_Pa$$word1!` |
| `club_admin` | club_admin | `SEED_CLUB_ADMIN_PASSWORD` | `ClubAdm1n!Passw0rd` |
| `member` | member | `SEED_MEMBER_PASSWORD` | `Member!User1Passw0rd` |

> **WARNING**: Never use these passwords outside local development.  For shared
> environments, set unique passwords via env vars or omit them to auto-generate.

## Authentication Quick Reference

```bash
# Get a CAPTCHA challenge (optional)
curl http://localhost:8080/api/v1/auth/captcha

# Login
curl -X POST http://localhost:8080/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"platform_admin","password":"Admin_Pa$$word1!"}'

# Use the returned token
TOKEN=<token from login response>
curl http://localhost:8080/api/v1/auth/session \
  -H "Authorization: Bearer $TOKEN"

# Logout
curl -X POST http://localhost:8080/api/v1/auth/logout \
  -H "Authorization: Bearer $TOKEN"
```

## Search Quick Reference

```bash
# Keyword search across candidates and offers (sorted by relevance score)
curl "http://localhost:8080/api/v1/search?q=rust&page=1&per_page=25" \
  -H "Authorization: Bearer $TOKEN"

# Filter by tags + sort by recency
curl "http://localhost:8080/api/v1/search?tags=backend,rust&sort_by=recency" \
  -H "Authorization: Bearer $TOKEN"

# Autocomplete suggestions
curl "http://localhost:8080/api/v1/search/autocomplete?prefix=eng&limit=10" \
  -H "Authorization: Bearer $TOKEN"

# Search history (current user)
curl "http://localhost:8080/api/v1/search/history?limit=20" \
  -H "Authorization: Bearer $TOKEN"

# Vocabulary categories + entries
curl "http://localhost:8080/api/v1/vocabularies" \
  -H "Authorization: Bearer $TOKEN"
curl "http://localhost:8080/api/v1/vocabularies/tags" \
  -H "Authorization: Bearer $TOKEN"
```

## Search Scoring

Results are scored in [0.0, 1.0] using a deterministic formula:

**Candidate score:**
```
score = (exact_match_bonus × 0.40)
      + (tag_overlap_ratio  × 0.35)
      + (recency_score       × 0.25)
```

**Offer score:**
```
score = (exact_match_bonus × 0.60)
      + (recency_score       × 0.40)
```

Where:
- `exact_match_bonus`: 1.0 if query exactly matches full name/email/title, else 0.0
- `tag_overlap_ratio`: `|requested_tags ∩ candidate_tags| / |requested_tags|`
- `recency_score`: linear decay over 365 days (1.0 at creation → 0.0 at 365+ days)

Ties broken by resource ID (lexicographic) to produce stable ordering.

**Sort options:** `?sort_by=relevance` (default), `?sort_by=recency`, `?sort_by=tag_overlap`, `?sort_by=popularity`, `?sort_by=rating`, `?sort_by=distance` (requires `site_code`; offers without coordinates sort last)

## Project Structure

```
src/
├── bin/                  # Binary entry points (server, seed)
├── api/
│   ├── handlers/         # Thin HTTP handlers (no business logic)
│   ├── routes/           # Route registration per resource group
│   └── middleware/       # Auth, request ID, rate limiting
├── application/          # Use-case / service layer
│   ├── user_service.rs       # User CRUD, role assignment/revocation
│   ├── booking_service.rs    # Booking lifecycle, holds, breach rules
│   ├── search_service.rs     # Search, scoring, autocomplete, spell-correction
│   ├── reporting_service.rs  # Subscriptions, alerts, dashboard versions
│   └── integration_service.rs # Connectors, sync state, import/export
├── domain/               # Domain models, enums, repository traits
│   ├── auth/
│   ├── users/
│   ├── candidates/
│   ├── offers/
│   ├── onboarding/
│   ├── bookings/
│   ├── search/
│   ├── reporting/
│   └── integrations/
├── infrastructure/       # External adapters
│   ├── config/           # Environment-driven configuration
│   ├── db/               # Connection pool, Diesel schema, repositories
│   ├── crypto/           # Password hashing, encryption, token generation
│   ├── logging/          # Structured JSON logging
│   ├── jobs/             # Background scheduler (hold release, session cleanup, daily reporting)
│   └── http/             # Outbound HTTP client
└── shared/               # Cross-cutting: errors, response envelope, pagination

migrations/               # Diesel SQL migrations (4 migration files)
seeds/                    # Seed data documentation
scripts/                  # Development and CI scripts
docs/                     # Architecture, API surface, traceability
unit_tests/               # Unit test modules (no DB required)
API_tests/                # Integration / API-level test modules (requires DB)
```

## Configuration

All configuration is via environment variables. See `docker-compose.yml` for the full list with defaults. All runtime is Docker-contained — no local Rust toolchain required.

| Variable                  | Required | Default         | Description                       |
|---------------------------|----------|-----------------|-----------------------------------|
| `DATABASE_URL`            | yes      | —               | PostgreSQL connection string      |
| `APP_HOST`                | no       | `127.0.0.1`     | Bind address                      |
| `APP_PORT`                | no       | `8080`          | Bind port                         |
| `ENCRYPTION_KEY`          | yes      | —               | Base64-encoded 32-byte AES key    |
| `STORAGE_PATH`            | no       | `./storage`     | Local file storage (import/export fallback) |
| `SESSION_TTL_SECONDS`     | no       | `28800`         | Session token lifetime (8 h)      |
| `SESSION_MAX_PER_USER`    | no       | `5`             | Max concurrent sessions per user  |
| `RATE_LIMIT_RPS`          | no       | `30`            | Requests per second limit         |
| `RATE_LIMIT_BURST`        | no       | `60`            | Burst allowance                   |
| `LOCKOUT_THRESHOLD`       | no       | `5`             | Failed login attempts before lock |
| `LOCKOUT_DURATION_SECONDS`| no       | `900`           | Account lockout duration          |
| `SCHEDULER_ENABLED`       | no       | `false`         | Enable background job scheduler   |
| `SNAPSHOT_TIMEZONE`       | no       | `UTC`           | IANA timezone for the daily snapshot (e.g. `America/New_York`) |
| `SNAPSHOT_TIME_LOCAL`     | no       | `06:00`         | Local wall-clock time `HH:MM` at which the daily snapshot fires |
| `RUN_MIGRATIONS`          | no       | `false`         | Run Diesel migrations on startup  |
| `RUN_SEED`                | no       | `false`         | Run seed script on startup        |
| `RUST_LOG`                | no       | `info`          | Log level filter                  |

## Background Scheduler Jobs

When `SCHEDULER_ENABLED=true`, three jobs run automatically:

| Job | Interval | Description |
|-----|----------|-------------|
| Hold auto-release | Every 60s | Releases expired `PendingConfirmation` booking holds |
| Session cleanup | Every 300s | Deletes expired session records |
| Daily reporting snapshot | `SNAPSHOT_TIME_LOCAL` in `SNAPSHOT_TIMEZONE` (default 06:00 UTC) | Runs active reporting subscriptions; fires alerts on threshold breaches |

The snapshot scheduler is DST-aware: it recomputes the next fire time after each run, so it always triggers at the configured wall-clock time regardless of daylight-saving transitions.

## Documentation

- [Architecture](docs/architecture.md)
- [API Surface](docs/api_surface.md)
- [Auth Model](docs/auth_model.md)
- [Business Modules](docs/business_modules.md)
- [Booking Module](docs/booking_module.md)
- [Requirement Traceability](docs/requirement_traceability.md)

## Running Tests

The recommended and policy-compliant way to run tests is via Docker — no local Rust toolchain required:

```bash
./run_tests.sh
```

To force Docker-based execution even when a local toolchain is installed:

```bash
./run_tests.sh --docker
```

The script:
1. Starts PostgreSQL in Docker
2. Builds the test image (`Dockerfile.test`)
3. Seeds the test database
4. Runs unit tests (no DB required)
5. Runs all API/integration test suites
6. Enforces a ≥90% coverage gate via `cargo tarpaulin`

**E2E tests** (against a live running server) are run separately:

```bash
./run_e2e.sh
```

**Coverage report only:**

```bash
./check_coverage.sh
```

## Verification

After `docker-compose up`:

```bash
# Health check
curl http://localhost:8080/api/v1/health

# Login and save token
TOKEN=$(curl -s -X POST http://localhost:8080/api/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"platform_admin","password":"Admin_Pa$$word1!"}' \
  | jq -r '.data.token')

# Search
curl "http://localhost:8080/api/v1/search?page=1&per_page=10" \
  -H "Authorization: Bearer $TOKEN"

# Vocabulary categories
curl "http://localhost:8080/api/v1/vocabularies" \
  -H "Authorization: Bearer $TOKEN"

# Create a reporting subscription
curl -X POST "http://localhost:8080/api/v1/reporting/subscriptions" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"report_type":"offers_expiring","parameters":{"days":7}}'

# List integration connectors
curl "http://localhost:8080/api/v1/integrations/connectors" \
  -H "Authorization: Bearer $TOKEN"
```

## Assumptions

1. All external delivery (email, webhook) is out of scope. Reporting alerts are stored locally and queried via the API.
2. The spell-correction dictionary is built from active controlled-vocabulary labels first, then recent historical queries. Vocabulary terms are always available; history augments them over time.
3. Integration sync is simulated as immediate success; actual HTTP transport to external systems is a future extension.
4. File-based import/export uses NDJSON written to `STORAGE_PATH`. The directory is created automatically.
5. `ENCRYPTION_KEY` in `docker-compose.yml` is a development-only key — replace with `$(openssl rand -base64 32)` before any deployment.

## License

Proprietary / Unlicensed
