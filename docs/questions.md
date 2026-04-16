# Business Logic Questions Log (Prompt Understanding)

This document records the business-level questions identified while interpreting the task prompt, along with the working hypothesis and the implemented solution observed in the current codebase under `repo/`.

## 1) Role boundary between `platform_admin`, `club_admin`, `member`, and `guest`

**Question:** The prompt defines a role hierarchy but does not fully specify whether `member` can mutate only their own records or must remain read-only for most resources.

**My Understanding/Hypothesis:** `platform_admin` is global, `club_admin` is delegated (optionally scoped), and `member/guest` should be constrained to self-owned scope with limited actions.

**Solution:** Implemented RBAC + object-level authorization. `platform_admin` has global access; `club_admin` has delegated domain operations; `member` access is restricted to own records in object-scoped services (for example candidate/offer/booking ownership checks). Permission checks are enforced in services via `AuthContext` and route middleware.

## 2) Club-admin scope model (global vs organization-scoped)

**Question:** Prompt mentions delegated operations (department recruiters) but does not define whether club admins are always tenant-scoped.

**My Understanding/Hypothesis:** Club admin can be either unscoped (global club admin) or scoped to an organization unit.

**Solution:** Added role-scope enforcement in auth context and service-level checks (notably candidate operations): scoped `club_admin` can only act on matching `organization_id`; unscoped club admin can operate globally.

## 3) “Own records” semantics for cross-resource entities

**Question:** Prompt says members/guests are limited to their own records, but does not define ownership for derived resources (offers, bookings, onboarding checklists).

**My Understanding/Hypothesis:** Ownership should resolve to `created_by` for primary resources and be inherited transitively for related workflows.

**Solution:** Services enforce owner-or-admin access rules per resource. Examples: booking access uses creator ownership checks; reporting subscriptions are owner-isolated unless caller is `platform_admin`.

## 4) Controlled vocabularies for cuisine/taste/price-like tags

**Question:** Prompt requires structured filters via organization-defined tags (cuisine/taste/price style), but does not prescribe schema categories.

**My Understanding/Hypothesis:** Use a generic controlled vocabulary table with category/value pairs and map domain filters onto those categories.

**Solution:** Implemented `controlled_vocabularies` with API access (`/vocabularies`, `/vocabularies/{category}`), plus validation in create/update paths. Search resolves vocabulary-backed category filters and applies them to candidates/offers deterministically.

## 5) Rating definition (source-of-truth vs derived score)

**Question:** Prompt requires rating filter/sort (`1.0–5.0`) but does not define if rating is user-provided, external, or computed.

**My Understanding/Hypothesis:** Rating can be derived when no native domain rating exists.

**Solution:** Search returns a `rating` field and supports `min_rating`/`max_rating` + sort-by-rating. Current implementation derives ratings from deterministic scoring (with domain-native offer salary signal when available, then fallback). Results without rating basis follow pass-through behavior for standard rating filters.

## 6) Distance search based on site codes without map lookups

**Question:** Prompt mandates miles-by-site-code distance, but does not define coordinate source or fallback behavior when coordinates are missing.

**My Understanding/Hypothesis:** Resolve `site_code` to stored office coordinates and compute local Haversine distance; do not call external map services.

**Solution:** Implemented local site repository + Haversine miles calculation in `SearchService`. If site/candidate coordinates are missing, `distance_miles` is omitted; distance sort places missing-distance records last; max-distance filter keeps records with no distance basis (pass-through).

## 7) Unified merged search ordering with “recommended” interleaving

**Question:** Prompt requires merged results interleaving recommended + matching records, but not exact ratio/tie-break policy.

**My Understanding/Hypothesis:** Interleaving must be deterministic and stable across repeated requests.

**Solution:** Implemented deterministic scoring and interleave policy: recommended threshold `score >= 0.5`, 3:1 interleave pattern, and stable tie-break ordering by sort field then deterministic identifiers.

## 8) Spell-correction + autocomplete dictionary composition

**Question:** Prompt says local dictionary from historical queries and approved tags, but not precedence or correction distance rules.

**My Understanding/Hypothesis:** Approved controlled-vocabulary terms should be primary; history should augment and personalize suggestions.

**Solution:** Implemented autocomplete from vocab + historical queries (deduped), and spell correction using local edit-distance matching (≤ 2). Query history is persisted for future suggestions.

## 9) Eligibility gate strictness before booking confirmation

**Question:** Prompt lists pre-booking checks but does not specify whether all checks are mandatory and how failures are reported.

**My Understanding/Hypothesis:** Confirmation requires all gate checks to pass; failures should return structured, explainable details.

**Solution:** Implemented 5-check eligibility gate in booking confirmation path. If any check fails, confirmation is blocked and a structured eligibility failure response is returned (with check-level detail).

## 10) “Checklist completion 100%” edge case with no required items

**Question:** Prompt requires 100% completion but does not define denominator behavior when required item count is zero.

**My Understanding/Hypothesis:** Zero required items should evaluate to 100% readiness (nothing blocking).

**Solution:** Readiness calculation treats `total_required = 0` as `100%`, while still allowing other gate checks (documents/restrictions/agreement/attestation freshness) to independently block booking confirmation.

## 11) Health/eligibility attestation freshness window

**Question:** Prompt says attestation must be signed within 30 days but does not define timestamp source or encryption handling.

**My Understanding/Hypothesis:** Attestation validity should use server-recorded completion timestamps and encrypted-at-rest content.

**Solution:** Onboarding items store encrypted health attestation data; eligibility checks enforce recency (`completed_at` within 30 days).

## 12) Hold expiration race (confirm near expiry)

**Question:** Prompt requires 15-minute holds and auto-release but does not define behavior when confirm arrives at/after expiry boundary.

**My Understanding/Hypothesis:** Confirmation should fail once hold expiry is reached, even if scheduler has not yet processed release.

**Solution:** Booking confirm path performs explicit `hold_expires_at` validation. Expired hold returns state-transition conflict and requires creating a new hold; scheduler separately performs periodic auto-release and inventory decrement.

## 13) Cancellation breach policy and reason-code enforcement

**Question:** Prompt says cancellations within 24 hours are breaches requiring reason code, but does not define response behavior when missing.

**My Understanding/Hypothesis:** Missing breach reason/reason_code in late-cancel should be validation failure.

**Solution:** Implemented 24-hour cutoff rules. Late cancellation requires `reason` + `reason_code`; otherwise request is rejected. Breach metadata is persisted and audited.

## 14) Reschedule semantics regarding agreement evidence

**Question:** Prompt allows reschedule up to 24 hours before start time, but does not state if prior agreement signature remains valid.

**My Understanding/Hypothesis:** Agreement evidence is context-bound to booking time/slot and should be re-collected after reschedule.

**Solution:** Reschedule flow releases old slot, reserves new slot, resets state to pending confirmation, and clears agreement evidence so a fresh agreement must be submitted.

## 15) Electronic agreement evidence model (typed name + timestamp + hash)

**Question:** Prompt mandates hash evidence but does not define hash payload composition.

**My Understanding/Hypothesis:** Hash should bind signer identity + timestamp + booking context to provide tamper evidence.

**Solution:** Agreement capture stores typed name, signed-at timestamp, and SHA-256 hash derived from booking context fields; confirmation requires evidence presence, not just a boolean flag.

## 16) Idempotency scope and conflict policy for write operations

**Question:** Prompt requires 24-hour dedupe but does not specify conflict behavior when same key is reused with different payload.

**My Understanding/Hypothesis:** Same key + same payload should replay; same key + different payload should fail with conflict.

**Solution:** Implemented canonical idempotency store with request hash, user/path scoping, replay on exact match, and `idempotency_conflict` on hash mismatch; TTL is 24 hours.

## 17) Integrations fallback when external connectors are unavailable

**Question:** Prompt requires graceful degradation to file-based import/export and incremental sync with `last_updated_at`, but does not define local format.

**My Understanding/Hypothesis:** Use local, append-friendly structured files for offline operation, and persist sync watermarks per connector.

**Solution:** Implemented connector model + sync-state/watermark tracking and file fallback import/export (NDJSON under local storage path), preserving offline operation with no third-party dependency.

## 18) Reporting schedule and timezone boundary behavior

**Question:** Prompt requires daily 6:00 AM local snapshots but does not clarify DST behavior.

**My Understanding/Hypothesis:** Scheduler should be wall-clock local-time aligned and DST-aware.

**Solution:** Implemented scheduled snapshot execution using configurable local timezone/time, with recomputation of next run to preserve wall-clock semantics across DST transitions.

## 19) Threshold alert formulas and ownership visibility

**Question:** Prompt gives examples (offers expiring in 7 days, breach rate >3% weekly) but does not define ownership visibility of subscriptions and alerts.

**My Understanding/Hypothesis:** Non-admins should only manage/view their own subscriptions and derived alerts.

**Solution:** Reporting service enforces owner isolation for get/update/delete/list operations, while `platform_admin` has global visibility. Threshold alert generation is supported in scheduled runs.

## 20) Sensitive data exposure defaults in API responses

**Question:** Prompt requires encryption at rest + masking by default, but does not define unmasking mechanism for authorized users.

**My Understanding/Hypothesis:** Default responses should never reveal sensitive fields; explicit opt-in flags plus permission checks are needed for reveal paths.

**Solution:** Candidate and offer responses are masked by default, with explicit reveal flags (`reveal_sensitive`, `reveal_compensation`) and service-layer authorization checks before decryption.

---

## Summary

The implementation resolves major prompt ambiguities by enforcing deterministic state machines, explicit object-level authorization, local-first search/ranking logic, 24-hour idempotency semantics, and offline-safe reporting/integration behavior.
