# TalentFlow Technical Design (Implementation-Aligned)

This document describes the **current implemented design** in `repo/` for the TalentFlow backend.

---

## 1. Purpose and Scope

TalentFlow is a single backend service that centralizes:

- Identity and session management
- User/role/permission administration
- Candidate profile management
- Offer and approval workflows
- Onboarding checklist/readiness workflows
- Booking/order lifecycle with inventory holds
- Unified search/discovery
- Reporting subscriptions and alerts
- Integration connectors with offline file fallback

The implementation is designed for **offline-runnable, single-host deployment** with local persistence and no required third-party network dependencies.

---

## 2. Technology Stack

- **Language:** Rust
- **HTTP framework:** Actix-web
- **Database access:** Diesel (typed query layer)
- **Database:** PostgreSQL
- **Container runtime:** Docker / Docker Compose
- **Async/background jobs:** Tokio + scheduler component in infrastructure layer

---

## 3. Architectural Style

The service is implemented as a **layered monolith**:

1. **API layer** (`src/api`)  
   Routes, middleware, DTO parsing/validation, response envelope shaping.

2. **Application layer** (`src/application`)  
   Use-case orchestration, business rules, authorization enforcement, idempotency handling, audit emission.

3. **Domain layer** (`src/domain`)  
   Domain models, enums, state transitions, repository contracts.

4. **Infrastructure layer** (`src/infrastructure`)  
   Diesel repositories, config loading, cryptography, scheduler, logging, connector execution.

5. **Shared layer** (`src/shared`)  
   Errors, response envelope, pagination, idempotency helpers.

### Design Intent

- Keep handlers thin and side-effect free beyond request/response mapping.
- Concentrate business policy in application/domain layers.
- Isolate persistence and external concerns behind repository/adaptor boundaries.

---

## 4. Core Domain Modules

### 4.1 Identity, Sessions, and RBAC

- Local username/password authentication only.
- Password complexity and secure hash verification.
- Failed login lockout policy with timed unlock.
- Session token persistence with expiry and cleanup.
- Role hierarchy implemented with permissions and optional scope constraints.
- Middleware-level authn + service-level authz checks.

### 4.2 Candidates

- CRUD over candidate records.
- Sensitive fields encrypted at rest (phone, SSN-last4 style fields).
- Masked-by-default responses with explicit reveal flags and permission checks.
- Tag validation against controlled vocabularies.

### 4.3 Offers and Approval Chain

- Offer creation/update/list/detail.
- Lifecycle transitions enforced by state rules.
- Ordered approval steps with assignee-driven decision updates.
- Compensation structure supported and encrypted at rest.

### 4.4 Onboarding

- Checklist and item management.
- Item status tracking and readiness percentage computation.
- Required-item completion influences booking eligibility.
- Health attestation data stored encrypted.

### 4.5 Bookings/Orders and Inventory

- Booking hold creation with 15-minute expiration.
- Slot reservation with transactional capacity protection (`SELECT ... FOR UPDATE` semantics in repository flow).
- Lifecycle transitions: pending confirmation → confirmed → in progress → completed, plus canceled/exception paths.
- Reschedule and cancellation rules with breach handling policy.
- Agreement evidence capture (typed name + timestamp + hash) before confirm.

### 4.6 Search and Discovery

- Unified candidate + offer search surface.
- Deterministic scoring and interleaving of recommended results.
- Sorting support including relevance/recency/popularity/rating/distance.
- Distance uses local site-code reference and local coordinates (no external map lookup).
- Autocomplete and spell-correction from controlled vocabulary + historical queries.

### 4.7 Reporting

- Subscription CRUD for report types.
- Ownership-isolated access for non-admin users.
- Daily snapshot scheduler support with local-time configuration.
- Alert persistence and dashboard version history.

### 4.8 Integrations

- Connector definitions for inbound/outbound/bidirectional usage.
- Encrypted connector auth config at rest.
- Incremental sync state/watermark tracking.
- Graceful fallback to local file import/export (offline-safe behavior).

---

## 5. Data and Persistence Design

### 5.1 Database Strategy

- PostgreSQL is the source of truth.
- Schema managed by Diesel migrations (`migrations/`).
- Repositories map DB models to domain/application contracts.

### 5.2 Key Persistence Themes

- **Auditable mutation paths:** mutation operations emit audit events.
- **Encrypted sensitive fields:** implemented in infrastructure crypto module.
- **Idempotent writes:** idempotency key tracking with request-hash comparison and TTL behavior.
- **Status-driven workflows:** enum-backed state transitions for offers/bookings.

### 5.3 Migration Footprint

The migration set includes:

- Initial schema
- Session idle-timeout support
- Business field expansion
- Booking workflow additions
- Reporting delivery metadata
- Candidate location support (distance features)
- Structured compensation fields

---

## 6. API and Contract Design

- Base path: `/api/v1`
- Resource-oriented REST-style endpoints across modules.
- Standard response envelope for success and errors.
- Validation failures map to structured error payloads.
- Mutating routes support idempotency key semantics.
- Pagination applied consistently on list endpoints.

See implemented route surface in `repo/docs/api_surface.md`.

---

## 7. Security Design

### 7.1 Authentication and Session Security

- Token-based session auth with expiry controls.
- Account lockout after repeated failed login attempts.
- CAPTCHA support for offline abuse mitigation.

### 7.2 Authorization

- Permission checks at operation boundaries.
- Object-level checks for ownership-sensitive resources.
- Admin override where explicitly allowed.

### 7.3 Data Protection

- AES-GCM style encryption for sensitive persisted fields.
- Masked responses by default for sensitive fields.
- Explicit reveal flags require permission + context checks.

### 7.4 Auditability

- Append-only style audit event pattern for key mutations and transitions.
- Designed to support traceability of permission-sensitive actions.

---

## 8. Workflow Design Highlights

### 8.1 Booking Confirmation Gate

Confirmation requires all relevant checks to pass, including:

- Onboarding completion constraints
- Required document/upload readiness
- Health/eligibility freshness constraints
- Restriction blocking checks
- Agreement evidence presence

Failure returns structured eligibility detail instead of silent state changes.

### 8.2 Hold Lifecycle

- Hold created with expiration timestamp.
- Scheduler releases expired holds and restores inventory availability.
- Confirmation path also validates hold freshness to prevent race-condition confirmations.

### 8.3 Search Determinism

- Scoring function is deterministic.
- Recommended interleaving follows a stable pattern.
- Tie-break behavior aims for repeatable ordering.

---

## 9. Background Jobs and Time-Based Behavior

When scheduler is enabled:

- Expired booking holds are released.
- Expired sessions are cleaned up.
- Reporting snapshot tasks run at configured local wall-clock time.

Timezone and scheduling are configuration-driven, with DST-aware behavior in reporting scheduling flow.

---

## 10. Observability and Operational Design

- Structured logging in infrastructure layer.
- Error model is normalized into API error envelope.
- Local metrics/log storage approach aligns with offline requirement.
- Runtime behavior and startup are controlled by env-driven config.

---

## 11. Deployment Model

- Primary deployment target: single host via Docker Compose.
- Development compose overlay supports migrations + seed workflows.
- Test flows include containerized integration execution and coverage gating scripts.

Operational scripts are provided in `repo/run_tests.sh`, `repo/check_coverage.sh`, and compose files.

---

## 12. Design Tradeoffs and Current Boundaries

1. **Layered monolith over microservices**  
   Chosen for offline simplicity and local operability.

2. **Local deterministic search over external intelligence**  
   Ensures zero third-party runtime dependency.

3. **File-based fallback for integrations**  
   Prioritizes resilience when external connectors are unavailable.

4. **Strict state transition control**  
   Reduces ambiguous lifecycle behavior at cost of tighter workflow constraints.

---

## 13. Traceability to Implementation Artifacts

Primary references already present in `repo/docs/`:

- `architecture.md` – layer boundaries and startup model
- `api_surface.md` – endpoint contract and API behavior
- `booking_module.md` – booking lifecycle and policy details
- `business_modules.md` – candidate/offer/onboarding module details
- `requirement_traceability.md` – requirement-to-code/test mapping

This `design.md` is the high-level design summary of those implemented artifacts.
