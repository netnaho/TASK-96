# TalentFlow — Static Delivery Acceptance + Architecture Audit

**Audit mode:** Static-only (no runtime execution)
**Date:** 2026-04-15
**Reviewer:** GitHub Copilot
**Repository root:** `repo/`

---

## 1) Delivery Acceptance Verdict

**Overall verdict: PARTIAL PASS (Not ready for unconditional acceptance).**

**Status scale used in this report:** `PASS` / `PARTIAL PASS` / `FAIL`

The repository shows substantial implementation breadth across auth, RBAC, candidate/offer/onboarding/booking domains, search/discovery, reporting, integrations, idempotency, and migrations. However, there are **material security and requirement-fit gaps** that should be addressed before final acceptance:

- **HIGH:** Reporting alert listing lacks owner scoping while `member` has `reporting:read`.
- **HIGH:** Session hard-expiry default is 1 hour, conflicting with the 8-hour idle timeout target.
- **HIGH:** CAPTCHA is documented as required near auth-IP limit but is explicitly optional in implementation.

No runtime claims are made in this report.

---

## 2) Scope, Constraints, and Method

### 2.1 Constraints honored

Per instruction, this review was static-only:

- No app startup
- No Docker execution
- No automated test execution
- No code changes to existing implementation

### 2.2 Method

Reviewed architecture, route wiring, middleware, handlers, service-layer logic, migrations, configuration, seed policy, and test suites for traceability and risk.

Primary evidence sources include:

- `src/api/routes/mod.rs`
- `src/api/middleware/auth.rs`
- `src/application/*_service.rs`
- `src/domain/auth/models.rs`
- `src/infrastructure/config/mod.rs`
- `src/infrastructure/reporting_delivery/mod.rs`
- `src/application/connector_executor.rs`
- `docs/requirement_traceability.md`
- `migrations/*/up.sql`
- `API_tests/*.rs`, `unit_tests/*.rs`

---

## 3) Architecture Mapping (Static)

### 3.1 Layering and boundaries

Layered architecture is clear and largely consistent:

- API layer (`routes`, `handlers`, `middleware`, `extractors`)
- Application services (business orchestration + authorization checks)
- Domain models/rules
- Infrastructure adapters (DB repositories, crypto, rate limit, jobs, logging, delivery)

### 3.2 Security boundary mapping

- Protected API scope wraps business modules under auth middleware: `src/api/routes/mod.rs:56-65`.
- Auth context exposes permission/scope/self-or-admin checks: `src/domain/auth/models.rs:84`, `:100`, `:125`.
- Reporting subscription object-level checks exist: `src/application/reporting_service.rs:186-200`, `:232`, `:260`.

### 3.3 Cross-cutting patterns

- Idempotency integrated in mutation paths (e.g., booking): `src/application/booking_service.rs:107-109`, `:123-136`, `:345-352`.
- Audit/event hooks and standardized envelopes are broadly present (static inspection of services/handlers).

---

## 4) Acceptance Review by Domain

### 4.1 Identity, Session, RBAC, Security Controls

**Status: PARTIAL PASS**

**What is implemented well**

- Session validation + touch of `last_activity_at`: `src/application/auth_service.rs:224-240`.
- Idle timeout constant defined at 8h: `src/application/auth_service.rs:41`, enforced at `:234`.
- Lockout enforcement exists: `src/application/auth_service.rs:127-130`.
- Protected route topology established: `src/api/routes/mod.rs:56-65`.
- Auth/IP/user rate limiter primitives exist: `src/infrastructure/ratelimit/mod.rs:71`, `:84`, `:97`.

**Material concerns**

- Hard expiry comes from `SESSION_TTL_SECONDS` defaulting to 3600 (1h):
  - Config default: `src/infrastructure/config/mod.rs:80`
  - Session creation uses this TTL: `src/application/auth_service.rs:160`
  - Compose also sets 3600: `docker-compose.yml:32`
- CAPTCHA is documented as required near auth-IP saturation, but implementation says optional:
  - `src/api/handlers/auth.rs:89-92`
  - Auth service validates captcha only when provided: `src/application/auth_service.rs:79-81`

### 4.2 Business Domain Workflows (Candidates/Offers/Onboarding/Bookings)

**Status: PASS (with minor notes)**

- Booking hold/agreement/confirm/cancel/reschedule/state flow present: `src/application/booking_service.rs:114`, `:244`, `:332`, `:495`, `:609`.
- Eligibility gate orchestration present: `src/application/eligibility_service.rs:37`.
- Object-level booking access checks used widely: `src/application/booking_service.rs:273`, `:368`, `:521`, `:635`, helper at `:927`.
- Role assignment/revocation admin-only at service layer: `src/application/user_service.rs:219`, `:285`.

### 4.3 Search & Discovery

**Status: PARTIAL PASS**

- Deterministic scoring/interleaving/recommendation are present.
- Distance/rating/popularity fields and filters are implemented.

Evidence:

- Derived rating is score-based: `src/application/search_service.rs:25`, `:194`, `:229`.
- Distance is Haversine from coordinates + `site_code`: `src/application/search_service.rs:27`, `:157`, `:201`, `:204`, `:554`.
- Distance filter passes through `None` values: `src/application/search_service.rs:262`.

Risk note: if product requirement expects business rating independent of relevance score, current implementation is semantically different.

### 4.4 Reporting

**Status: FAIL (security-sensitive gap)**

- Subscription ownership isolation is implemented for get/update/delete/list-subscriptions:
  - `src/application/reporting_service.rs:186-200`, `:232`, `:260`
- Dashboard publish requires update permission: `src/application/reporting_service.rs:282`

**Gap:** Alert listing does not apply owner scoping; it only checks permission then lists alerts globally:

- `src/application/reporting_service.rs:338-339`

Because seeded `member` role has `reporting:read`:

- `src/bin/seed.rs:123`

this creates potential cross-user alert data exposure unless repository-level filtering exists by other means (not evident in service call signature).

### 4.5 Integrations & Delivery Connectors

**Status: PARTIAL PASS**

- Connector execution supports HTTP sync + file fallback.
- HTTP paths constrain to `http://` scheme (no TLS):
  - Connector executor: `src/application/connector_executor.rs:192`, `:196-197`
  - Reporting delivery gateway: `src/infrastructure/reporting_delivery/mod.rs:256`, `:262-263`

Risk note: “local-network-only” is documented, but no strict host allowlist/private-range enforcement is visible.

### 4.6 Data Model & Migrations

**Status: PASS**

Migrations cover core surfaces including session idle timeout, booking workflow, reporting delivery metadata, candidate location, and structured compensation fields:

- `migrations/00000000000002_session_idle_timeout/up.sql`
- `migrations/00000000000004_booking_workflow/up.sql`
- `migrations/00000000000005_reporting_delivery_meta/up.sql`
- `migrations/00000000000006_candidate_location/up.sql`
- `migrations/00000000000007_compensation_structured_fields/up.sql`

---

## 5) Issues & Recommendations (Severity-ranked)

### 5.1 HIGH — Cross-user alert visibility risk in reporting alerts

**Evidence**

- Alert list only checks `reporting:read`, no owner filter in service call: `src/application/reporting_service.rs:338-339`
- Member is granted reporting read permission: `src/bin/seed.rs:123`

**Impact**

Potential tenant/user data exposure through alerts.

**Recommendation**

Apply owner scoping similar to subscriptions:

- Non-admin: `WHERE subscription.user_id = ctx.user_id`
- Admin: full visibility

Add explicit API tests for alert list ownership isolation.

---

### 5.2 HIGH — Session hard-expiry default (1h) conflicts with 8h idle-timeout behavior

**Evidence**

- Config TTL default 3600s: `src/infrastructure/config/mod.rs:80`
- Session expiry set from this TTL at login: `src/application/auth_service.rs:160`
- Idle timeout separately set to 8h: `src/application/auth_service.rs:41`, enforced at `:234`
- Compose enforces 3600 in default env: `docker-compose.yml:32`

**Impact**

Users may be logged out around 1 hour even when active, violating 8-hour idle expectations.

**Recommendation**

Align defaults to product requirement (e.g., 8h), or clearly document dual-policy semantics and set compose defaults accordingly.

---

### 5.3 HIGH — CAPTCHA enforcement gap (optional where docs imply conditional requirement)

**Evidence**

- Handler docs: required near auth-IP limit but “optional” currently: `src/api/handlers/auth.rs:89-92`
- Auth service validates captcha only if provided: `src/application/auth_service.rs:79-81`

**Impact**

Brute-force pressure mitigation weaker than stated policy.

**Recommendation**

Introduce explicit enforcement condition (e.g., failed-login threshold or auth-IP token threshold) and add integration tests for mandatory CAPTCHA path.

---

### 5.4 MEDIUM — Role/permission catalog enumeration available to any authenticated user

**Evidence**

- Handler accepts auth but no permission gate: `src/api/handlers/users.rs:385-421`
- Service explicitly describes any-authenticated visibility: `src/application/user_service.rs:329-335`

**Impact**

Increases internal attack surface reconnaissance (permission model discovery).

**Recommendation**

Restrict to privileged roles (e.g., platform admin / security admin), or explicitly justify as intended behavior.

---

### 5.5 MEDIUM — “Local-network-only” connector/delivery claim lacks strict host boundary enforcement

**Evidence**

- Documentation claims local network only: `src/infrastructure/reporting_delivery/mod.rs:5`
- Runtime only enforces `http://` scheme, not host scope: `src/infrastructure/reporting_delivery/mod.rs:262-263`, `src/application/connector_executor.rs:196-197`

**Impact**

Misconfiguration can target arbitrary HTTP hosts.

**Recommendation**

Add URL validation/allowlist/private-range checks and configuration guardrails.

---

### 5.6 LOW — Seeded known credentials + auto-seed in compose defaults

**Evidence**

- Fixed default credentials in seed script: `src/bin/seed.rs:27-29`
- Compose enables seeding by default: `docker-compose.yml:40`

**Impact**

Risk if non-dev environment reuses compose defaults.

**Recommendation**

Gate seeding to explicit dev profile and force credential override in non-local environments.

---

## 6) Security Summary

### 6.1 Strengths

- Clear auth boundary and middleware-wrapped protected scope.
- Service-level object authorization present in multiple domains.
- Lockout + rate limiting + encrypted sensitive fields + idempotency patterns are present.

### 6.2 Priority risks

1. Reporting alert visibility scoping gap (HIGH)
2. Session policy mismatch (HIGH)
3. CAPTCHA enforcement gap (HIGH)

### 6.3 Security acceptance position

**Status: FAIL**

Not security-clean for final acceptance until HIGH items are remediated and covered by tests.

---

## 7) Static Review of Tests and Logging

### 7.1 Test surface (static inspection only)

Strong breadth exists across unit and API suites.

Evidence examples:

- Alert auth test exists but no explicit alert ownership isolation test observed:
  - `API_tests/search_reporting_tests.rs:333` (`test_list_alerts_requires_auth`)
- Subscription ownership isolation is tested:
  - `API_tests/search_reporting_tests.rs:554` (`test_subscription_access_isolation`)
- Member reporting permissions tested for dashboard/ack paths:
  - `API_tests/search_reporting_tests.rs:1072`, `:1093`
- Search behavior tests include rating/Haversine/recommended/interleave:
  - `unit_tests/search_tests.rs:217`, `:238`, `:399`, `:303`

### 7.2 Logging/observability

Structured logging and warn/info tracing appear throughout service code.
No runtime logging behavior is asserted here (static-only constraint).

---

## 8) Mandatory Static Coverage Assessment

### 8.1 Requirement-to-implementation coverage snapshot

- Identity/Auth/RBAC: **PARTIAL PASS** (implemented with notable policy mismatch: session TTL / CAPTCHA enforcement).
- Core business workflows: **PASS**.
- Search/discovery: **PARTIAL PASS** (implemented, with semantic caveat on rating meaning).
- Reporting: **FAIL** (implemented, but likely alert visibility gap).
- Integrations: **PARTIAL PASS** (implemented, with host-boundary hardening needed).
- Migrations/schema: **PASS** (comprehensive and aligned to features).

### 8.2 High-risk coverage gap matrix

| Risk area                             | Static code evidence                                                                        | Test evidence found                                                                                                         | Coverage judgment                               |
| ------------------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| Alert ownership isolation             | `src/application/reporting_service.rs:338-339`                                              | No explicit `GET /reporting/alerts` owner-isolation test found; auth-only test at `API_tests/search_reporting_tests.rs:333` | **Gap**                                         |
| CAPTCHA mandatory enforcement trigger | `src/api/handlers/auth.rs:89-92`, `src/application/auth_service.rs:79-81`                   | Wrong-answer validation exists (`API_tests/auth_tests.rs:270`)                                                              | **Gap**                                         |
| Session hard-expiry vs idle policy    | `src/infrastructure/config/mod.rs:80`, `src/application/auth_service.rs:160`, `:41`, `:234` | No policy-alignment test found in static scan                                                                               | **Gap**                                         |
| Search distance/rating semantics      | `src/application/search_service.rs:25`, `:27`, `:194`, `:204`, `:554`                       | Unit/API tests present for implemented semantics (`unit_tests/search_tests.rs:217`, `:238`; API search tests)               | **Covered for current code, but spec-fit risk** |

---

## 9) Final Notes and Acceptance Conditions

### 9.1 Final disposition

This codebase is **architecturally substantial and close to acceptance**, but should not be accepted as fully compliant until the HIGH findings are resolved.

### 9.2 Minimum acceptance conditions

1. Enforce owner/admin scoping for `GET /reporting/alerts`.
2. Align session expiry defaults/policy with 8-hour idle expectation.
3. Implement real conditional CAPTCHA requirement (not optional-only).
4. Add targeted API tests for the above.

### 9.3 Optional hardening follow-ups

- Restrict role/permission catalog endpoints to privileged roles.
- Add URL host-boundary enforcement for connector/delivery endpoints.
- Restrict/guard seeding defaults for safer non-local deployments.

---

## Requirements coverage summary

- Static-only audit constraints respected: **Done**
- Evidence-based findings with traceability: **Done**
- Security-first prioritization with blocker/high emphasis: **Done**
- Consolidated markdown artifact in `./.tmp/**.md`: **Done** (`.tmp/talentflow_static_delivery_architecture_audit.md`)
