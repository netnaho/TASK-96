# Unified Test Coverage + README Audit Report (Strict Static Mode)

Execution mode: **static inspection only** (no code/tests/scripts/containers executed).

---

## Test Coverage Audit

### Backend Endpoint Inventory

Source of truth:

- Prefix + scope wiring: `src/api/routes/mod.rs::configure`
- Endpoint declarations: `src/api/routes/{auth,health,users,candidates,offers,onboarding,bookings,search,reporting,integrations,audit}.rs`

Resolved API version/prefix: **`/api/v1`**

Total unique endpoints (`METHOD + PATH`): **69**

|   # | Method | Path                                                 |
| --: | ------ | ---------------------------------------------------- |
|   1 | GET    | `/api/v1/health`                                     |
|   2 | POST   | `/api/v1/auth/login`                                 |
|   3 | GET    | `/api/v1/auth/captcha`                               |
|   4 | POST   | `/api/v1/auth/logout`                                |
|   5 | GET    | `/api/v1/auth/session`                               |
|   6 | GET    | `/api/v1/users`                                      |
|   7 | POST   | `/api/v1/users`                                      |
|   8 | GET    | `/api/v1/users/{id}`                                 |
|   9 | PUT    | `/api/v1/users/{id}`                                 |
|  10 | GET    | `/api/v1/users/{id}/roles`                           |
|  11 | POST   | `/api/v1/users/{id}/roles`                           |
|  12 | DELETE | `/api/v1/users/{id}/roles/{role_id}`                 |
|  13 | GET    | `/api/v1/roles`                                      |
|  14 | GET    | `/api/v1/permissions`                                |
|  15 | GET    | `/api/v1/candidates`                                 |
|  16 | POST   | `/api/v1/candidates`                                 |
|  17 | GET    | `/api/v1/candidates/{id}`                            |
|  18 | PUT    | `/api/v1/candidates/{id}`                            |
|  19 | GET    | `/api/v1/offers`                                     |
|  20 | POST   | `/api/v1/offers`                                     |
|  21 | GET    | `/api/v1/offers/{id}`                                |
|  22 | PUT    | `/api/v1/offers/{id}`                                |
|  23 | POST   | `/api/v1/offers/{id}/submit`                         |
|  24 | POST   | `/api/v1/offers/{id}/withdraw`                       |
|  25 | GET    | `/api/v1/offers/{id}/approvals`                      |
|  26 | POST   | `/api/v1/offers/{id}/approvals`                      |
|  27 | PUT    | `/api/v1/offers/{id}/approvals/{step_id}`            |
|  28 | GET    | `/api/v1/onboarding/checklists`                      |
|  29 | POST   | `/api/v1/onboarding/checklists`                      |
|  30 | GET    | `/api/v1/onboarding/checklists/{id}`                 |
|  31 | GET    | `/api/v1/onboarding/checklists/{id}/items`           |
|  32 | POST   | `/api/v1/onboarding/checklists/{id}/items`           |
|  33 | PUT    | `/api/v1/onboarding/checklists/{id}/items/{item_id}` |
|  34 | POST   | `/api/v1/bookings`                                   |
|  35 | GET    | `/api/v1/bookings`                                   |
|  36 | GET    | `/api/v1/bookings/{id}`                              |
|  37 | POST   | `/api/v1/bookings/{id}/agreement`                    |
|  38 | POST   | `/api/v1/bookings/{id}/confirm`                      |
|  39 | POST   | `/api/v1/bookings/{id}/start`                        |
|  40 | POST   | `/api/v1/bookings/{id}/complete`                     |
|  41 | POST   | `/api/v1/bookings/{id}/cancel`                       |
|  42 | POST   | `/api/v1/bookings/{id}/reschedule`                   |
|  43 | POST   | `/api/v1/bookings/{id}/exception`                    |
|  44 | GET    | `/api/v1/sites`                                      |
|  45 | GET    | `/api/v1/sites/{id}`                                 |
|  46 | GET    | `/api/v1/search`                                     |
|  47 | GET    | `/api/v1/search/autocomplete`                        |
|  48 | GET    | `/api/v1/search/history`                             |
|  49 | GET    | `/api/v1/vocabularies`                               |
|  50 | GET    | `/api/v1/vocabularies/{category}`                    |
|  51 | GET    | `/api/v1/reporting/subscriptions`                    |
|  52 | POST   | `/api/v1/reporting/subscriptions`                    |
|  53 | GET    | `/api/v1/reporting/subscriptions/{id}`               |
|  54 | PUT    | `/api/v1/reporting/subscriptions/{id}`               |
|  55 | DELETE | `/api/v1/reporting/subscriptions/{id}`               |
|  56 | GET    | `/api/v1/reporting/dashboards/{key}/versions`        |
|  57 | POST   | `/api/v1/reporting/dashboards/{key}/versions`        |
|  58 | GET    | `/api/v1/reporting/alerts`                           |
|  59 | PUT    | `/api/v1/reporting/alerts/{id}/acknowledge`          |
|  60 | GET    | `/api/v1/integrations/connectors`                    |
|  61 | POST   | `/api/v1/integrations/connectors`                    |
|  62 | GET    | `/api/v1/integrations/connectors/{id}`               |
|  63 | PUT    | `/api/v1/integrations/connectors/{id}`               |
|  64 | POST   | `/api/v1/integrations/connectors/{id}/sync`          |
|  65 | GET    | `/api/v1/integrations/connectors/{id}/sync-state`    |
|  66 | POST   | `/api/v1/integrations/import`                        |
|  67 | POST   | `/api/v1/integrations/export`                        |
|  68 | GET    | `/api/v1/audit`                                      |
|  69 | GET    | `/api/v1/audit/{id}`                                 |

### API Test Mapping Table

Legend:

- `TNM HTTP` = true no-mock HTTP
- `N/A` = no endpoint-level HTTP request found

| Endpoint                                                 | Covered | Type     | Test files                                                                                                                                                                | Evidence                                                                                   |
| -------------------------------------------------------- | ------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| GET `/api/v1/health`                                     | yes     | TNM HTTP | `API_tests/auth_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                      | `health_check_returns_200` (`auth_tests.rs`), `health_check_returns_200` (`http_tests.rs`) |
| POST `/api/v1/auth/login`                                | yes     | TNM HTTP | all API suites + e2e                                                                                                                                                      | `TestRequest::post().uri("/api/v1/auth/login")` / `reqwest.post(.../auth/login)`           |
| GET `/api/v1/auth/captcha`                               | yes     | TNM HTTP | `API_tests/auth_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                      | captcha tests in both files                                                                |
| POST `/api/v1/auth/logout`                               | yes     | TNM HTTP | `API_tests/auth_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                      | logout tests                                                                               |
| GET `/api/v1/auth/session`                               | yes     | TNM HTTP | `API_tests/auth_tests.rs`, `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                       | session access tests                                                                       |
| GET `/api/v1/users`                                      | yes     | TNM HTTP | `API_tests/auth_tests.rs`, `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                       | list/forbidden tests                                                                       |
| POST `/api/v1/users`                                     | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | user create test                                                                           |
| GET `/api/v1/users/{id}`                                 | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | get user tests                                                                             |
| PUT `/api/v1/users/{id}`                                 | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | update user test                                                                           |
| GET `/api/v1/users/{id}/roles`                           | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | list roles test                                                                            |
| POST `/api/v1/users/{id}/roles`                          | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | assign role tests                                                                          |
| DELETE `/api/v1/users/{id}/roles/{role_id}`              | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | revoke role tests                                                                          |
| GET `/api/v1/roles`                                      | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | roles endpoint tests                                                                       |
| GET `/api/v1/permissions`                                | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | permissions endpoint tests                                                                 |
| GET `/api/v1/candidates`                                 | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | candidates pagination/list tests                                                           |
| POST `/api/v1/candidates`                                | yes     | TNM HTTP | `API_tests/business_tests.rs`, `API_tests/booking_tests.rs`, `API_tests/scheduler_integration_tests.rs`, `API_tests/search_reporting_tests.rs`, `e2e_tests/http_tests.rs` | candidate create flows                                                                     |
| GET `/api/v1/candidates/{id}`                            | yes     | TNM HTTP | `e2e_tests/http_tests.rs`                                                                                                                                                 | get candidate by id                                                                        |
| PUT `/api/v1/candidates/{id}`                            | yes     | TNM HTTP | `e2e_tests/http_tests.rs`                                                                                                                                                 | update candidate                                                                           |
| GET `/api/v1/offers`                                     | yes     | TNM HTTP | `e2e_tests/http_tests.rs`                                                                                                                                                 | list offers                                                                                |
| POST `/api/v1/offers`                                    | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | create offer                                                                               |
| GET `/api/v1/offers/{id}`                                | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | get offer                                                                                  |
| PUT `/api/v1/offers/{id}`                                | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | update offer                                                                               |
| POST `/api/v1/offers/{id}/submit`                        | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | submit offer tests                                                                         |
| POST `/api/v1/offers/{id}/withdraw`                      | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | withdraw tests                                                                             |
| GET `/api/v1/offers/{id}/approvals`                      | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | approvals list test                                                                        |
| POST `/api/v1/offers/{id}/approvals`                     | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | approval-step create test                                                                  |
| PUT `/api/v1/offers/{id}/approvals/{step_id}`            | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | approval decision test                                                                     |
| GET `/api/v1/onboarding/checklists`                      | yes     | TNM HTTP | `e2e_tests/http_tests.rs`                                                                                                                                                 | checklist list test                                                                        |
| POST `/api/v1/onboarding/checklists`                     | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | create checklist tests                                                                     |
| GET `/api/v1/onboarding/checklists/{id}`                 | **no**  | N/A      | —                                                                                                                                                                         | no direct GET on this exact path found in API/E2E tests                                    |
| GET `/api/v1/onboarding/checklists/{id}/items`           | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | list checklist items tests                                                                 |
| POST `/api/v1/onboarding/checklists/{id}/items`          | yes     | TNM HTTP | `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                                  | create item tests                                                                          |
| PUT `/api/v1/onboarding/checklists/{id}/items/{item_id}` | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | update item test                                                                           |
| POST `/api/v1/bookings`                                  | yes     | TNM HTTP | `API_tests/booking_tests.rs`, `API_tests/scheduler_integration_tests.rs`, `e2e_tests/http_tests.rs`                                                                       | create/hold tests                                                                          |
| GET `/api/v1/bookings`                                   | yes     | TNM HTTP | `API_tests/booking_tests.rs`, `API_tests/scheduler_integration_tests.rs`, `e2e_tests/http_tests.rs`                                                                       | list bookings tests                                                                        |
| GET `/api/v1/bookings/{id}`                              | yes     | TNM HTTP | `API_tests/booking_tests.rs`, `API_tests/scheduler_integration_tests.rs`                                                                                                  | get booking tests                                                                          |
| POST `/api/v1/bookings/{id}/agreement`                   | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | agreement tests                                                                            |
| POST `/api/v1/bookings/{id}/confirm`                     | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | confirm booking tests                                                                      |
| POST `/api/v1/bookings/{id}/start`                       | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | start transition tests                                                                     |
| POST `/api/v1/bookings/{id}/complete`                    | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | complete transition tests                                                                  |
| POST `/api/v1/bookings/{id}/cancel`                      | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | cancel rule tests                                                                          |
| POST `/api/v1/bookings/{id}/reschedule`                  | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | reschedule test                                                                            |
| POST `/api/v1/bookings/{id}/exception`                   | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | exception test                                                                             |
| GET `/api/v1/sites`                                      | yes     | TNM HTTP | `API_tests/booking_tests.rs`, `API_tests/search_reporting_tests.rs`                                                                                                       | sites list tests                                                                           |
| GET `/api/v1/sites/{id}`                                 | yes     | TNM HTTP | `API_tests/booking_tests.rs`                                                                                                                                              | get site and not-found tests                                                               |
| GET `/api/v1/search`                                     | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                          | search requests with filters/sorts                                                         |
| GET `/api/v1/search/autocomplete`                        | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | autocomplete test                                                                          |
| GET `/api/v1/search/history`                             | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | history test                                                                               |
| GET `/api/v1/vocabularies`                               | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | vocabulary list                                                                            |
| GET `/api/v1/vocabularies/{category}`                    | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | unknown-category 404                                                                       |
| GET `/api/v1/reporting/subscriptions`                    | yes     | TNM HTTP | `e2e_tests/http_tests.rs`                                                                                                                                                 | list subscriptions                                                                         |
| POST `/api/v1/reporting/subscriptions`                   | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`, `API_tests/business_tests.rs`, `e2e_tests/http_tests.rs`                                                                           | create subscription tests                                                                  |
| GET `/api/v1/reporting/subscriptions/{id}`               | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | get own/forbidden/not-found                                                                |
| PUT `/api/v1/reporting/subscriptions/{id}`               | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | update subscription test                                                                   |
| DELETE `/api/v1/reporting/subscriptions/{id}`            | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | delete subscription test                                                                   |
| GET `/api/v1/reporting/dashboards/{key}/versions`        | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | list dashboard versions                                                                    |
| POST `/api/v1/reporting/dashboards/{key}/versions`       | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | publish dashboard tests                                                                    |
| GET `/api/v1/reporting/alerts`                           | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                          | list alerts tests                                                                          |
| PUT `/api/v1/reporting/alerts/{id}/acknowledge`          | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | acknowledge alert tests                                                                    |
| GET `/api/v1/integrations/connectors`                    | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | connectors list tests                                                                      |
| POST `/api/v1/integrations/connectors`                   | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`, `API_tests/business_tests.rs`                                                                                                      | create connector tests                                                                     |
| GET `/api/v1/integrations/connectors/{id}`               | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | get connector tests                                                                        |
| PUT `/api/v1/integrations/connectors/{id}`               | yes     | TNM HTTP | `API_tests/business_tests.rs`                                                                                                                                             | update connector test                                                                      |
| POST `/api/v1/integrations/connectors/{id}/sync`         | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | sync trigger tests                                                                         |
| GET `/api/v1/integrations/connectors/{id}/sync-state`    | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | sync-state tests                                                                           |
| POST `/api/v1/integrations/import`                       | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | import fallback test                                                                       |
| POST `/api/v1/integrations/export`                       | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | export tests                                                                               |
| GET `/api/v1/audit`                                      | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`, `e2e_tests/http_tests.rs`                                                                                                          | audit list tests                                                                           |
| GET `/api/v1/audit/{id}`                                 | yes     | TNM HTTP | `API_tests/search_reporting_tests.rs`                                                                                                                                     | audit get by id                                                                            |

### API Test Classification

1. **True No-Mock HTTP**

- `API_tests/auth_tests.rs`
- `API_tests/booking_tests.rs`
- `API_tests/business_tests.rs`
- `API_tests/search_reporting_tests.rs`
- `API_tests/scheduler_integration_tests.rs`
- `e2e_tests/http_tests.rs`

Evidence:

- In-process HTTP app bootstrap in API suites: `test::init_service(App::new()... .configure(routes::configure))`
- Real HTTP client in e2e suite: `reqwest::Client` against `E2E_BASE_URL`

2. **HTTP with Mocking**

- **None found** (strict pattern scan)

3. **Non-HTTP (unit/integration without HTTP)**

- All modules declared in `unit_tests/mod.rs` (`agreement_tests`, `search_tests`, `booking_state_machine_tests`, etc.)
- Also direct service/repository calls inside `API_tests/scheduler_integration_tests.rs` (e.g., `BookingService::release_expired_holds`) used as supplementary integration checks.

### Mock Detection

Patterns inspected: `jest.mock`, `vi.mock`, `sinon.stub`, mocked transport/provider patterns.

Result: **No mocking/stubbing framework usage detected** in API/E2E test suites.

Notes:

- Occurrences of `fake_id` are synthetic UUID test data, not mocks (`API_tests/search_reporting_tests.rs`).

### Coverage Summary

- Total endpoints: **69**
- Endpoints with HTTP tests: **68**
- Endpoints with TRUE no-mock tests: **68**

Computed:

- HTTP coverage = **98.55%** ($68/69$)
- True API coverage = **98.55%** ($68/69$)

Uncovered endpoint:

- `GET /api/v1/onboarding/checklists/{id}`

### Unit Test Summary

Unit test files (from `unit_tests/mod.rs`):

- `agreement_tests`
- `auth_context_tests`
- `booking_rules_tests`
- `booking_state_machine_tests`
- `captcha_tests`
- `compensation_tests`
- `error_tests`
- `export_staging_tests`
- `idempotency_logic_tests`
- `integration_sync_tests`
- `offer_state_machine_tests`
- `password_tests`
- `readiness_tests`
- `reporting_delivery_tests`
- `response_envelope_tests`
- `scheduler_tests`
- `search_tests`
- `seed_config_tests`

Module coverage characterization:

- Controllers/handlers: covered mostly via HTTP API/E2E, not dedicated unit tests
- Services/domain logic: strong unit coverage for state machines, scheduler logic, search logic, idempotency, validation helpers
- Repositories: mostly integration-tested through API tests with DB setup/assertions
- Auth/guards/middleware: strong HTTP coverage via `401/403/session/logout` tests

Important modules not clearly unit-tested as dedicated modules:

- `src/application/user_service.rs`
- `src/application/candidate_service.rs`
- `src/application/onboarding_service.rs` (direct unit module not explicit)
- Route registration modules (`src/api/routes/*.rs`) have no dedicated unit-only tests

### Tests Check

`run_tests.sh` findings:

- Docker-based path: **OK**
- Local dependency path exists (`cargo fmt/clippy/test` branch): **FLAG**

Evidence:

- `run_tests.sh` local branch: `if command -v cargo ...` then local cargo commands
- Docker path exists in `else` branch

### API Observability Check

Strong observability examples:

- `API_tests/booking_tests.rs`: method/path + JSON payload + response field assertions
- `API_tests/search_reporting_tests.rs`: filters/sorts + response schema assertions
- `e2e_tests/http_tests.rs`: real HTTP requests with status and body checks

Weak spots:

- Some tests assert only status code (limited response-content verification), e.g. selected search/auth checks in `API_tests/search_reporting_tests.rs` and `API_tests/auth_tests.rs`.

### Test Quality & Sufficiency

- Success paths: broad coverage across auth, CRUD, workflows, reporting, integrations
- Failure/validation: present (401/403/404/409/422)
- Edge cases: present (idempotency replay/conflict, pagination extremes, rating/distance filters)
- Auth/permissions: strongly covered (role matrix, ownership checks)
- Integration boundaries: good (API + e2e + DB-backed setup)

Remaining sufficiency gap:

- Missing endpoint-specific coverage for `GET /api/v1/onboarding/checklists/{id}`
- Some superficial assertions (status-only) reduce confidence on contract integrity

### End-to-End Expectations

- Project appears backend API (README top: `Type: backend`; no FE app in inspected scope)
- E2E suite exists (`e2e_tests/http_tests.rs`) using real HTTP (`reqwest`) against live base URL
- Fullstack FE↔BE E2E not required for backend-only project

### Test Coverage Score (0–100)

**91 / 100**

### Score Rationale

- High endpoint coverage (68/69)
- Strong true no-mock HTTP testing posture
- Good depth on business-critical flows and permission boundaries
- Deductions for one uncovered endpoint and mixed assertion depth
- Process deduction for local-toolchain path in `run_tests.sh` (strict policy signal)

### Key Gaps

1. Uncovered endpoint: `GET /api/v1/onboarding/checklists/{id}`
2. Inconsistent assertion depth in some tests (status-only)
3. `run_tests.sh` includes local cargo path (strict test-process flag)

### Confidence & Assumptions

Confidence: **High** for endpoint inventory and request-path mapping; **Medium-High** for quality scoring.

Assumptions:

1. Actix in-process request tests are accepted as real HTTP-layer tests (middleware + router + handlers executed).
2. Coverage determined by visible method+path requests in test code; static-only (no runtime branch confirmation).
3. No hidden/generated tests outside inspected folders.

**Test Coverage Verdict: PARTIAL PASS**

---

## README Audit

Target file: `repo/README.md`

### Project Type Detection

- Declared at top: **`Type: backend`**
- Inference not needed.

### README Location

- `repo/README.md` exists: **PASS**

### Hard Gate Checks

1. Formatting/readability: **PASS**
2. Startup instruction includes required `docker-compose up`: **PASS**
   - Evidence: Quick Start section includes both `docker-compose -f ... up` and `docker-compose up`
3. Access method (URL + port): **PASS**
   - Evidence: `http://localhost:8080` examples
4. Verification method present: **PASS**
   - Evidence: explicit verification curl flows in `## Verification`
5. Environment rules (Docker-contained; no manual runtime installs/setup): **PASS**
   - Evidence: README states Docker-first, and no `npm install`/`pip install`/`apt-get`/manual DB setup steps shown
6. Demo credentials with auth and all roles: **PASS**
   - Evidence: `platform_admin`, `club_admin`, `member` credentials table

### High Priority Issues

- None.

### Medium Priority Issues

1. README states Docker-first, but repository script `run_tests.sh` still supports local cargo path (alignment issue between docs policy and script behavior).
   - Evidence: `run_tests.sh` local branch guarded by `command -v cargo`.

### Low Priority Issues

1. Default seed credentials are clearly marked dev-only but still highly sensitive; risk remains if copied to non-dev environments.

### Hard Gate Failures

- None.

### README Verdict

**PASS**

---

## Final Verdicts

- **Test Coverage Audit:** PARTIAL PASS
- **README Audit:** PASS

Combined strict outcome: **PARTIAL PASS** (blocked from full pass by test-coverage gaps).
