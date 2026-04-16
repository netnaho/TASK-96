# Delivery Acceptance & Project Architecture Audit — TASK-96 (Static-Only)

## 1) Audit Context, Scope, and Method

- **Mode:** Static analysis only (no runtime execution, no Docker, no tests executed, no code changes).
- **Codebase audited:** `repo/` (Rust Actix + Diesel/Postgres).
- **Evidence rule:** Every substantive claim cites `file:line` where possible.

## 2) Overall Verdict

**Decision: Partial Pass (Not production-acceptance ready).**

**Judgment scale used in this report:** `Pass` / `Partial Pass` / `Fail`.

The codebase demonstrates strong baseline architecture and many security controls, but there are unresolved **authorization and requirement-fit gaps** that should be addressed before final acceptance.

- **Blocker:** 0
- **High:** 2
- **Medium:** 3
- **Low:** 1

## 3) Acceptance Area A — API Surface & Functional Delivery

### Status: **Partial Pass**

### What is implemented well

- Protected/public route segregation is explicit; authenticated surface is wrapped under auth middleware (`src/api/routes/mod.rs:49`, `src/api/routes/mod.rs:56`, `src/api/routes/mod.rs:57`).
- Search API supports paging/sorting and additive filters (`src/api/handlers/search.rs:19`, `src/api/handlers/search.rs:29`, `src/api/handlers/search.rs:31`, `src/api/handlers/search.rs:32`).
- Unified search result includes ranking and recommendation fields (`src/application/search_service.rs:66`, `src/application/search_service.rs:194`, `src/application/search_service.rs:229`).

### Gaps / defects

1. **[Medium] Export feature appears functionally incomplete**
   - `ExportInput` exposes `field_map`, but `export_data` returns `records_exported: 0` in both connector and file paths and does not apply mapping in shown logic (`src/application/integration_service.rs:106`, `src/application/integration_service.rs:587`, `src/application/integration_service.rs:606`).
   - Risk: API contract suggests export transformation/output count, but implementation behaves as staging metadata only.

2. **[Medium] Search requirement-fit risk (domain-specific filters not represented)**
   - Query model supports `q/tags/status/min_rating/max_rating/max_distance_miles/site_code` only (`src/api/handlers/search.rs:20`, `src/api/handlers/search.rs:21`, `src/api/handlers/search.rs:22`, `src/api/handlers/search.rs:29`, `src/api/handlers/search.rs:30`, `src/api/handlers/search.rs:31`, `src/api/handlers/search.rs:32`).
   - Derived `rating` is computed from relevance score (`score * 5`) rather than explicit persisted user/business rating (`src/application/search_service.rs:194`, `src/application/search_service.rs:229`).
   - Risk: If acceptance criteria expected business-native dimensions (e.g., cuisine/taste/price or user-sourced ratings), current model likely does not satisfy them.

## 4) Acceptance Area B — Architecture & Layering

### Status: **Pass**

- Clear route/middleware → application service → repository layering is present (route wiring in `src/api/routes/mod.rs:49`, `src/api/routes/mod.rs:56`; service-to-repo usage e.g. reporting list/acknowledge paths in `src/application/reporting_service.rs:358`, `src/application/reporting_service.rs:364`).
- Configuration is centralized and typed; lockout/session/rate-limit settings are environment-driven (`src/infrastructure/config/mod.rs:94`).
- Audit model uses immutable event storage with DB-level mutation prevention (`migrations/00000000000001_initial_schema/up.sql:362`, `migrations/00000000000001_initial_schema/up.sql:369`, `migrations/00000000000001_initial_schema/up.sql:373`).

## 5) Acceptance Area C — Authorization, Object Isolation, and Security

### Status: **Partial Pass (critical issues remain)**

### Strengths

- Auth boundary is fail-closed by design (`src/api/routes/mod.rs:43`).
- IP-level throttling exists before auth checks (`src/api/routes/mod.rs:23`, `src/api/middleware/rate_limit.rs:75`, `src/api/middleware/rate_limit.rs:77`).
- Lockout + CAPTCHA escalation is implemented (`src/application/auth_service.rs:112`, `src/application/auth_service.rs:113`, `src/application/auth_service.rs:175`).
- Outbound local-network guard exists with allowlist and tests (`src/shared/network.rs:25`, `src/shared/network.rs:28`, `src/shared/network.rs:66`).

### High-severity findings

1. **[High] Self-service user endpoints allow broad `club_admin` bypass**
   - `require_self_or_admin` allows access if caller has `club_admin`, independent of ownership (`src/domain/auth/models.rs:125`, `src/domain/auth/models.rs:128`).
   - `get_user`, `update_user`, and `list_user_roles` rely on that helper (`src/application/user_service.rs:78`, `src/application/user_service.rs:162`, `src/application/user_service.rs:209`).
   - Risk: horizontal access to unrelated user data/profile operations by any `club_admin` unless this is explicitly intended in acceptance policy.

2. **[High] Alert acknowledgment lacks explicit ownership/scope enforcement**
   - Service checks only `reporting:update` then acknowledges by alert id (`src/application/reporting_service.rs:358`, `src/application/reporting_service.rs:363`, `src/application/reporting_service.rs:364`).
   - Repository update is scoped only by alert `id` (no owner/subscription join constraint) (`src/infrastructure/db/repositories/reporting_repo.rs:297`).
   - Risk: any actor with `reporting:update` can acknowledge alerts outside intended ownership boundary.

## 6) Acceptance Area D — Data Model, Persistence, and Auditability

### Status: **Pass with caveats**

- Append-only audit table and DB triggers prevent updates/deletes (`migrations/00000000000001_initial_schema/up.sql:362`, `migrations/00000000000001_initial_schema/up.sql:369`, `migrations/00000000000001_initial_schema/up.sql:373`, `migrations/00000000000001_initial_schema/up.sql:375`).
- Idempotency key persistence and expiry are defined in schema (`migrations/00000000000001_initial_schema/up.sql:330`).

**Caveat:** ownership constraints for alert acknowledgment are enforced weakly at service/repo boundary (see High finding above).

## 7) Acceptance Area E — Configuration, Operability, and Documentation

### Status: **Partial Pass**

- Session model is documented in auth service comments (`src/application/auth_service.rs:18`, `src/application/auth_service.rs:20`, `src/application/auth_service.rs:52`).
- **[Low] Config/documentation default mismatch:** README states `SESSION_TTL_SECONDS` default `3600` (`README.md:211`), while config default is `28800` (`src/infrastructure/config/mod.rs:94`).
- Risk: operational misconfiguration, incorrect hard-expiry assumptions during deployment.

## 8) Mandatory Security and Test-Coverage Audits

### 8.1 Security-Focused Audit Summary

- **Authn controls:** lockout + CAPTCHA + idle/hard expiry present (`src/application/auth_service.rs:112`, `src/application/auth_service.rs:175`, `src/application/auth_service.rs:209`, `src/application/auth_service.rs:283`).
- **Transport/egress guardrails:** local/private network validation for gateways/connectors (`src/shared/network.rs:25`, `src/shared/network.rs:28`, `src/shared/network.rs:66`).
- **Key residual risks:**
  - User object-level overreach via `club_admin` path (`src/domain/auth/models.rs:125`, `src/application/user_service.rs:78`).
  - Alert ownership bypass on acknowledge (`src/application/reporting_service.rs:363`, `src/infrastructure/db/repositories/reporting_repo.rs:297`).

### 8.2 Static Test Coverage Assessment (Required)

| Risk / Requirement Area | Evidence of Tests | Coverage Judgment |
|---|---|---|
| Auth context role/self checks | `unit_tests/auth_context_tests.rs:54`, `unit_tests/auth_context_tests.rs:65`, `unit_tests/auth_context_tests.rs:68` | **Covered**, but current tests also normalize the `club_admin` bypass behavior. |
| Reporting subscription isolation | `API_tests/search_reporting_tests.rs:536`, `API_tests/search_reporting_tests.rs:580` | **Covered** for read/delete isolation scenarios. |
| Reporting alert permission gate | `API_tests/search_reporting_tests.rs:1211` | **Partially covered** (permission denial for member), **not** owner-scope acknowledge matrix. |
| Integrations import/export API | Import assertion exists (`API_tests/search_reporting_tests.rs:470`); export endpoint reachable (`API_tests/search_reporting_tests.rs:488`, `API_tests/search_reporting_tests.rs:493`) | **Partial**; no assertion that export applies `field_map` or produces non-zero exports. |
| Search filters/sorting contracts | Sort/rating/distance tests in `API_tests/search_reporting_tests.rs` (e.g., `test_search_sort_by_rating`, `test_search_with_min_rating_filter`) | **Covered** for current contract, not for potential missing business-native filter dimensions. |
| Audit immutability | No explicit static test found in reviewed test files for DB trigger mutation rejection | **Gap** (schema-level control exists, but no explicit test evidence found in reviewed test set). |

## 9) Consolidated Issues, Severity, and Required Actions

### High priority (must fix before final acceptance)

1. **[High] Tighten user object authorization semantics**
   - Replace/augment `require_self_or_admin` usage for profile/role reads with explicit scope-aware checks where required.
   - Evidence: `src/domain/auth/models.rs:125`, `src/application/user_service.rs:78`, `src/application/user_service.rs:162`, `src/application/user_service.rs:209`.

2. **[High] Enforce owner/scope checks in alert acknowledge path**
   - Ensure `acknowledge_alert` validates alert’s subscription ownership (or platform-admin override) before update.
   - Evidence: `src/application/reporting_service.rs:363`, `src/infrastructure/db/repositories/reporting_repo.rs:297`.

### Medium priority

3. **[Medium] Implement/export real mapped records (or narrow API contract)**
   - Align `field_map` and `records_exported` behavior with endpoint semantics.
   - Evidence: `src/application/integration_service.rs:106`, `src/application/integration_service.rs:587`, `src/application/integration_service.rs:606`.

4. **[Medium] Reconcile search feature set with acceptance vocabulary**
   - If requirements include business-native facets (e.g., cuisine/taste/price), add canonical fields and persisted scoring source.
   - Evidence: `src/api/handlers/search.rs:19`, `src/api/handlers/search.rs:29`, `src/application/search_service.rs:194`.

5. **[Medium] Add missing tests for alert-ownership acknowledge and export semantics**
   - Permission-only acknowledge test currently exists, but no ownership matrix (`API_tests/search_reporting_tests.rs:1211`).
   - Export path is hit with `field_map`, but no mapped-output assertions (`API_tests/search_reporting_tests.rs:488`, `API_tests/search_reporting_tests.rs:493`).

### Low priority

6. **[Low] Fix README/session TTL default mismatch**
   - Evidence: `README.md:211` vs `src/infrastructure/config/mod.rs:94`.

---

## Requirements Coverage Summary

- **Static-only audit constraint:** **Done** (no runtime execution performed).
- **Evidence-based findings with citations:** **Done**.
- **Security-focused acceptance analysis:** **Done** (Section 8.1 + High findings).
- **Mandatory static test coverage assessment:** **Done** (Section 8.2).
- **Consolidated markdown report in `.tmp`:** **Done** (`.tmp/delivery_acceptance_architecture_audit_TASK-96.md`).
