# TalentFlow API Specification (Implementation-Aligned)

This specification reflects the currently implemented API in `repo/`.

- **Base URL (local):** `http://localhost:8080`
- **API base path:** `/api/v1`
- **Primary content type:** `application/json`
- **Auth scheme:** Bearer token (`Authorization: Bearer <token>`)

---

## 1) Conventions

### 1.1 Response Envelope

#### Success (single)

```json
{
  "data": {},
  "meta": {
    "request_id": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

#### Success (list/paginated)

```json
{
  "data": [],
  "pagination": {
    "page": 1,
    "per_page": 25,
    "total": 142
  },
  "meta": {
    "request_id": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

#### Error

```json
{
  "error": {
    "code": "validation_error",
    "message": "validation failed",
    "details": [
      { "field": "email", "message": "must be a valid email address" }
    ]
  }
}
```

### 1.2 Error Codes

| Code                       | HTTP | Meaning                                     |
| -------------------------- | ---: | ------------------------------------------- |
| `validation_error`         |  422 | Request validation failed                   |
| `authentication_required`  |  401 | Missing/invalid auth                        |
| `forbidden`                |  403 | Permission denied                           |
| `not_found`                |  404 | Resource does not exist                     |
| `conflict`                 |  409 | Generic business conflict                   |
| `invalid_state_transition` |  409 | Invalid lifecycle transition                |
| `idempotency_conflict`     |  409 | Same idempotency key with different payload |
| `rate_limited`             |  429 | Request throttled                           |
| `internal_error`           |  500 | Unexpected server error                     |

### 1.3 Pagination

Most list endpoints support:

- `page` (default `1`)
- `per_page` (default `25`, max `100`)

### 1.4 Idempotency

All mutating endpoints (`POST`, `PUT`, and transition sub-routes) support optional `Idempotency-Key`.

Behavior:

- Same key + same body/hash => original response replay
- Same key + different body/hash => `409 idempotency_conflict`
- No key => normal processing

Canonical dedupe window is 24 hours.

---

## 2) Authentication and Session APIs

### `GET /health`

- **Auth:** No
- **Description:** Liveness/readiness ping

### `GET /auth/captcha`

- **Auth:** No
- **Description:** Issue offline CAPTCHA challenge token/payload

### `POST /auth/login`

- **Auth:** No
- **Description:** Authenticate user and create session
- **Typical request body:**

```json
{
  "username": "platform_admin",
  "password": "Admin_Pa$$word1!"
}
```

- **Typical response data fields:** session token, expiry/session metadata, role context

### `POST /auth/logout`

- **Auth:** Yes
- **Description:** Revoke current session

### `GET /auth/session`

- **Auth:** Yes
- **Description:** Inspect current session and effective auth context

---

## 3) Users, Roles, Permissions

### Authorization model notes

- User-management writes are admin-restricted.
- `GET /users/{id}` and `GET /users/{id}/roles` allow self-access.
- `GET /roles` and `GET /permissions` are available to authenticated users.

### Endpoints

| Method | Path                          | Auth | Description      |
| ------ | ----------------------------- | ---- | ---------------- |
| GET    | `/users`                      | Yes  | List users       |
| POST   | `/users`                      | Yes  | Create user      |
| GET    | `/users/{id}`                 | Yes  | Get user         |
| PUT    | `/users/{id}`                 | Yes  | Update user      |
| GET    | `/users/{id}/roles`           | Yes  | List user roles  |
| POST   | `/users/{id}/roles`           | Yes  | Assign role      |
| DELETE | `/users/{id}/roles/{role_id}` | Yes  | Revoke role      |
| GET    | `/roles`                      | Yes  | List roles       |
| GET    | `/permissions`                | Yes  | List permissions |

---

## 4) Candidate APIs

Sensitive candidate fields are masked by default.

### Endpoints

| Method | Path               | Auth | Description          |
| ------ | ------------------ | ---- | -------------------- |
| GET    | `/candidates`      | Yes  | List candidates      |
| POST   | `/candidates`      | Yes  | Create candidate     |
| GET    | `/candidates/{id}` | Yes  | Get candidate detail |
| PUT    | `/candidates/{id}` | Yes  | Update candidate     |

### Query parameters

- `page`, `per_page` on list
- `reveal_sensitive=true` on detail (permission-gated)

---

## 5) Offer and Approval APIs

Offer updates are lifecycle-constrained (for example draft-only updates).

### Offer endpoints

| Method | Path                    | Auth | Description                        |
| ------ | ----------------------- | ---- | ---------------------------------- |
| GET    | `/offers`               | Yes  | List offers                        |
| POST   | `/offers`               | Yes  | Create offer                       |
| GET    | `/offers/{id}`          | Yes  | Get offer detail                   |
| PUT    | `/offers/{id}`          | Yes  | Update offer                       |
| POST   | `/offers/{id}/submit`   | Yes  | Transition offer for approval flow |
| POST   | `/offers/{id}/withdraw` | Yes  | Withdraw offer                     |

### Approval endpoints

| Method | Path                               | Auth | Description         |
| ------ | ---------------------------------- | ---- | ------------------- |
| GET    | `/offers/{id}/approvals`           | Yes  | List approval steps |
| POST   | `/offers/{id}/approvals`           | Yes  | Add step            |
| PUT    | `/offers/{id}/approvals/{step_id}` | Yes  | Record decision     |

### Query parameters

- `page`, `per_page`, optional `candidate_id` on list
- `reveal_compensation=true` on detail (permission-gated)

---

## 6) Onboarding APIs

### Endpoints

| Method | Path                                          | Auth | Description                |
| ------ | --------------------------------------------- | ---- | -------------------------- |
| GET    | `/onboarding/checklists`                      | Yes  | List checklists            |
| POST   | `/onboarding/checklists`                      | Yes  | Create checklist           |
| GET    | `/onboarding/checklists/{id}`                 | Yes  | Get checklist              |
| GET    | `/onboarding/checklists/{id}/items`           | Yes  | List items                 |
| POST   | `/onboarding/checklists/{id}/items`           | Yes  | Add item                   |
| PUT    | `/onboarding/checklists/{id}/items/{item_id}` | Yes  | Update item status/content |

### Query parameters

- `page`, `per_page`, optional `candidate_id` on checklist listing

---

## 7) Booking and Site APIs

Booking workflow enforces hold, eligibility, and transition rules.

### Booking endpoints

| Method | Path                        | Auth | Description                         |
| ------ | --------------------------- | ---- | ----------------------------------- |
| POST   | `/bookings`                 | Yes  | Create hold on slot                 |
| GET    | `/bookings`                 | Yes  | List bookings                       |
| GET    | `/bookings/{id}`            | Yes  | Get booking                         |
| POST   | `/bookings/{id}/agreement`  | Yes  | Submit agreement evidence           |
| POST   | `/bookings/{id}/confirm`    | Yes  | Confirm (runs eligibility gate)     |
| POST   | `/bookings/{id}/start`      | Yes  | Confirmed -> InProgress             |
| POST   | `/bookings/{id}/complete`   | Yes  | InProgress/Exception -> Completed   |
| POST   | `/bookings/{id}/cancel`     | Yes  | Cancel (breach rules apply)         |
| POST   | `/bookings/{id}/reschedule` | Yes  | Move to new slot (rule-constrained) |
| POST   | `/bookings/{id}/exception`  | Yes  | Mark exception                      |

### Site endpoints

| Method | Path          | Auth | Description       |
| ------ | ------------- | ---- | ----------------- |
| GET    | `/sites`      | Yes  | List active sites |
| GET    | `/sites/{id}` | Yes  | Get site detail   |

---

## 8) Search and Vocabulary APIs

Unified search returns interleaved candidate + offer results with deterministic ordering.

### Endpoints

| Method | Path                       | Auth | Description                |
| ------ | -------------------------- | ---- | -------------------------- |
| GET    | `/search`                  | Yes  | Unified resource search    |
| GET    | `/search/autocomplete`     | Yes  | Suggest terms              |
| GET    | `/search/history`          | Yes  | Caller query history       |
| GET    | `/vocabularies`            | Yes  | List vocabulary categories |
| GET    | `/vocabularies/{category}` | Yes  | List values for category   |

### `GET /search` query parameters

| Param                | Type       | Notes                                                                               |
| -------------------- | ---------- | ----------------------------------------------------------------------------------- |
| `q`                  | string     | Keyword text                                                                        |
| `tags`               | csv string | Candidate tag overlap filter                                                        |
| `status`             | string     | Offer status filter                                                                 |
| `sort_by`            | string     | `relevance` (default), `recency`, `tag_overlap`, `popularity`, `rating`, `distance` |
| `page`               | int        | Default `1`                                                                         |
| `per_page`           | int        | Default `25`, max `100`                                                             |
| `min_rating`         | float      | Lower bound, pass-through on no rating basis                                        |
| `max_rating`         | float      | Upper bound, pass-through on no rating basis                                        |
| `max_distance_miles` | float      | Distance upper bound, pass-through on no distance basis                             |
| `site_code`          | string     | Required to compute distance-based values                                           |

### Search result item shape (high level)

| Field              | Type            | Notes                                   |
| ------------------ | --------------- | --------------------------------------- |
| `resource_type`    | string          | `candidate` or `offer`                  |
| `id`               | UUID            | Resource ID                             |
| `title`            | string          | Display title                           |
| `subtitle`         | string/null     | Secondary text                          |
| `score`            | float           | Deterministic relevance score           |
| `tags`             | array           | Candidate tags (offers typically empty) |
| `status`           | string/null     | Offer status when applicable            |
| `created_at`       | ISO-8601 string | Creation timestamp                      |
| `rating`           | float?          | Optional derived rating                 |
| `popularity_score` | float?          | Optional derived popularity metric      |
| `distance_miles`   | float?          | Optional site-relative distance         |
| `recommended`      | bool            | High-confidence result marker           |

### Search response extras

- Optional `spell_correction` value when correction suggestion exists.

---

## 9) Reporting APIs

### Endpoints

| Method | Path                                   | Auth | Description               |
| ------ | -------------------------------------- | ---- | ------------------------- |
| GET    | `/reporting/subscriptions`             | Yes  | List subscriptions        |
| POST   | `/reporting/subscriptions`             | Yes  | Create subscription       |
| GET    | `/reporting/subscriptions/{id}`        | Yes  | Get subscription          |
| PUT    | `/reporting/subscriptions/{id}`        | Yes  | Update subscription       |
| DELETE | `/reporting/subscriptions/{id}`        | Yes  | Delete subscription       |
| GET    | `/reporting/dashboards/{key}/versions` | Yes  | List dashboard versions   |
| POST   | `/reporting/dashboards/{key}/versions` | Yes  | Publish dashboard version |
| GET    | `/reporting/alerts`                    | Yes  | List alerts               |
| PUT    | `/reporting/alerts/{id}/acknowledge`   | Yes  | Acknowledge alert         |

### Report types

| Type              | Purpose                      | Typical parameters                           |
| ----------------- | ---------------------------- | -------------------------------------------- |
| `offers_expiring` | Upcoming offer expiry alerts | `{ "days": 7 }`                              |
| `breach_rate`     | Breach threshold alerting    | `{ "threshold_pct": 3.0, "window_days": 7 }` |
| `snapshot`        | Daily summary snapshot       | `{}`                                         |

### Ownership rules

- Non-admin users are restricted to their own subscriptions/associated visibility.
- `platform_admin` can access all.

---

## 10) Integration APIs

### Endpoints

| Method | Path                                       | Auth | Description                            |
| ------ | ------------------------------------------ | ---- | -------------------------------------- |
| GET    | `/integrations/connectors`                 | Yes  | List connectors                        |
| POST   | `/integrations/connectors`                 | Yes  | Create connector                       |
| GET    | `/integrations/connectors/{id}`            | Yes  | Get connector                          |
| PUT    | `/integrations/connectors/{id}`            | Yes  | Update connector                       |
| POST   | `/integrations/connectors/{id}/sync`       | Yes  | Trigger sync                           |
| GET    | `/integrations/connectors/{id}/sync-state` | Yes  | Get sync state/watermark               |
| POST   | `/integrations/import`                     | Yes  | Import from connector or file fallback |
| POST   | `/integrations/export`                     | Yes  | Export via connector or file fallback  |

### Connector types

- `inbound`
- `outbound`
- `bidirectional`

`auth_config` is encrypted at rest and not returned in clear form in API responses.

---

## 11) Audit APIs

Read-only audit access.

| Method | Path          | Auth | Description       |
| ------ | ------------- | ---- | ----------------- |
| GET    | `/audit`      | Yes  | List audit events |
| GET    | `/audit/{id}` | Yes  | Get audit event   |

No create/update/delete audit endpoints are exposed.

---

## 12) Authentication Header and Common HTTP Patterns

### Auth header

```http
Authorization: Bearer <token>
```

### Idempotency header (mutations)

```http
Idempotency-Key: <client-generated-unique-key>
```

### Typical status usage

- `200 OK` - successful read/update/action
- `201 Created` - successful resource creation
- `401 Unauthorized` - missing/invalid token
- `403 Forbidden` - permission or ownership mismatch
- `404 Not Found` - unknown resource
- `409 Conflict` - lifecycle conflict/idempotency conflict
- `422 Unprocessable Entity` - validation or eligibility-style failure
- `429 Too Many Requests` - throttled requests

---

## 13) Notes for Consumers

1. Treat optional fields (`rating`, `distance_miles`, `spell_correction`, etc.) as nullable/omittable depending on basis.
2. Always send idempotency keys for create/update/transition writes to avoid accidental duplicate mutations.
3. For state-machine resources (offers/bookings), expect `409 invalid_state_transition` when invoking an action from a disallowed state.
4. For sensitive resources, assume masked defaults and use explicit reveal options only where supported and authorized.
