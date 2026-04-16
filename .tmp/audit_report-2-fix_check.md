# Fix Verification — Audit Report 2 (TASK-96)

## Scope & Method

- **Input baseline:** `.tmp/audit_report-2.md`
- **Verification mode:** static code + test inspection only (no runtime execution)
- **Target:** determine whether previously reported issues are fixed in current code

## Overall Re-check Verdict

**Result: Mostly Fixed (5 fixed, 1 partially fixed).**

All prior **High** and **Low** findings are fixed in code. Medium findings were addressed, with one requirement-fit item still partially dependent on business acceptance interpretation.

---

## Issue-by-issue Verification

### 1) [High] Self-service endpoints allowed broad `club_admin` bypass

**Previous finding:** `get_user` / `update_user` / `list_user_roles` used `require_self_or_admin`, allowing cross-user access for any `club_admin`.

**Current status:** ✅ **Fixed**

**Evidence:**

- Strict check introduced to exclude `club_admin` bypass:
  - `src/domain/auth/models.rs:142`–`src/domain/auth/models.rs:144`
  - `src/domain/auth/models.rs:151`
- User service endpoints now use strict method:
  - `src/application/user_service.rs:78`
  - `src/application/user_service.rs:162`
  - `src/application/user_service.rs:209`
- Unit tests added for strict behavior:
  - `unit_tests/auth_context_tests.rs:94`
  - `unit_tests/auth_context_tests.rs:110`
  - `unit_tests/auth_context_tests.rs:130`

---

### 2) [High] Alert acknowledge lacked explicit ownership/scope enforcement

**Previous finding:** `acknowledge_alert` checked permission only, repo update by alert `id`.

**Current status:** ✅ **Fixed**

**Evidence:**

- Service now resolves alert + subscription and enforces owner check (non-platform-admin):
  - `src/application/reporting_service.rs:363`
  - `src/application/reporting_service.rs:367`
  - `src/application/reporting_service.rs:371`
  - `src/application/reporting_service.rs:373`
- Ownership matrix tests now present:
  - `API_tests/search_reporting_tests.rs:1760` (owner allowed)
  - `API_tests/search_reporting_tests.rs:1806` (non-owner club_admin denied)
  - `API_tests/search_reporting_tests.rs:1851` (platform_admin allowed)

**Note:** Repository update remains id-based (`src/infrastructure/db/repositories/reporting_repo.rs:308`), but this is now safely gated by service-level ownership enforcement.

---

### 3) [Medium] Export semantics (`field_map`, `records_exported`) appeared incomplete

**Previous finding:** Export returned `records_exported: 0` and seemed not to apply mapping.

**Current status:** ✅ **Fixed by explicit contract narrowing (staging-only design)**

**Evidence:**

- Export API now documents staging-only semantics and explicitly states `records_exported` is always `0` until `trigger_sync`:
  - `src/application/integration_service.rs:530`
  - `src/application/integration_service.rs:537`
  - `src/application/integration_service.rs:541`
- `field_map` is persisted in NDJSON `_meta` header:
  - `src/application/integration_service.rs:600`
  - `src/application/integration_service.rs:685`
  - `src/application/integration_service.rs:701`
- API tests now validate this contract:
  - `API_tests/search_reporting_tests.rs:1901`
  - `API_tests/search_reporting_tests.rs:1921`
  - `API_tests/search_reporting_tests.rs:1937`
  - `API_tests/search_reporting_tests.rs:1953`

---

### 4) [Medium] Search requirement-fit risk (missing domain-native filters/ratings)

**Previous finding:** limited query dimensions and relevance-derived rating risk.

**Current status:** ⚠️ **Partially Fixed (substantially improved)**

**Evidence of improvements:**

- Search now includes business-native optional facets (`department`, `source`, `salary_*`, `categories`, `price_*`, `quality_*`):
  - Handler query model: `src/api/handlers/search.rs:34`–`src/api/handlers/search.rs:47`
  - Service input and filtering: `src/application/search_service.rs:108`–`src/application/search_service.rs:131`, `src/application/search_service.rs:282`–`src/application/search_service.rs:286`, `src/application/search_service.rs:396`–`src/application/search_service.rs:399`
- Offer rating now prefers domain-native salary-based logic when available:
  - `src/application/search_service.rs:351`
  - `src/application/search_service.rs:355`
  - `src/application/search_service.rs:580`
  - `src/application/search_service.rs:786`

**Residual caveat:**

- Candidates still use relevance-derived rating fallback (no persisted domain rating source):
  - `src/application/search_service.rs:342`

So this is a strong mitigation, but final acceptance still depends on whether product requirements require domain-native rating for **all** resource types.

---

### 5) [Medium] Missing tests for alert-ownership acknowledge and export semantics

**Current status:** ✅ **Fixed**

**Evidence:**

- Alert ownership tests added:
  - `API_tests/search_reporting_tests.rs:1760`
  - `API_tests/search_reporting_tests.rs:1806`
  - `API_tests/search_reporting_tests.rs:1851`
- Export semantics tests added:
  - `API_tests/search_reporting_tests.rs:1901`
  - `API_tests/search_reporting_tests.rs:1937`

---

### 6) [Low] README/session TTL default mismatch

**Current status:** ✅ **Fixed**

**Evidence:**

- README now lists `SESSION_TTL_SECONDS` default `28800`:
  - `README.md:211`
- Config default is `28800`:
  - `src/infrastructure/config/mod.rs:94`

---

## Additional check (from prior coverage caveat)

### Audit immutability tests

**Status:** ✅ **Now covered**

**Evidence:**

- Update rejection test:
  - `API_tests/business_tests.rs:1594`
- Delete rejection test:
  - `API_tests/business_tests.rs:1629`

---

## Summary Table

| Prior Issue                                  | Status Now      | Notes                                                            |
| -------------------------------------------- | --------------- | ---------------------------------------------------------------- |
| High: self-service `club_admin` overreach    | Fixed           | strict self/platform-admin checks wired into user endpoints      |
| High: alert acknowledge ownership bypass     | Fixed           | ownership check added in service + ownership matrix tests        |
| Medium: export semantics mismatch            | Fixed           | contract explicitly staging-only; tests align                    |
| Medium: search requirement-fit risk          | Partially Fixed | major facet/rating improvements; candidate rating still fallback |
| Medium: missing tests (ack ownership/export) | Fixed           | tests added                                                      |
| Low: README TTL mismatch                     | Fixed           | README and config now aligned                                    |

## Final Re-check Conclusion

The previously reported blockers for acceptance are largely resolved in the current code. The only remaining item is a **requirement interpretation caveat** around whether candidate-side rating must be domain-native (vs relevance-derived fallback). If the acceptance criteria allow mixed rating provenance, this would be considered fully acceptable.
