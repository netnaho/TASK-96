# Seed Data

Seed logic lives in `src/bin/seed.rs` and is executed as a standalone binary.

## Startup Flow

In the Docker Compose environment:
1. PostgreSQL starts and passes healthcheck
2. The app binary runs pending Diesel migrations (if `RUN_MIGRATIONS=true`)
3. The seed binary runs **only if `RUN_SEED=true`** (default: `false`)

Seeding is **off by default** in the base `docker-compose.yml`.  For local
development, use the dev override:

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml up
```

For local development without Docker:
```bash
# Set seed passwords (or omit to auto-generate random ones)
export SEED_ADMIN_PASSWORD='Admin_Pa$$word1!'
export SEED_CLUB_ADMIN_PASSWORD='ClubAdm1n!Passw0rd'
export SEED_MEMBER_PASSWORD='Member!User1Passw0rd'
cargo run --bin seed
```

## Seed User Passwords

Passwords are **never hardcoded**.  They are resolved from environment variables
at seed time.  If a variable is not set, a random password is generated and
printed to stdout exactly once.

| Env var                      | Username        | Default behavior    |
|------------------------------|-----------------|---------------------|
| `SEED_ADMIN_PASSWORD`        | platform_admin  | random if unset     |
| `SEED_CLUB_ADMIN_PASSWORD`   | club_admin      | random if unset     |
| `SEED_MEMBER_PASSWORD`       | member          | random if unset     |

The `docker-compose.dev.yml` override sets these to known values for local
development convenience.

> **WARNING**: Never use known/static seed passwords in shared, staging, or
> production environments.  Always use unique, randomly generated passwords or
> set explicit values via the environment variables above.

## What Gets Seeded

All seed operations use `INSERT ... ON CONFLICT DO NOTHING` and are idempotent.

### Roles
| ID | Name | System |
|----|------|--------|
| `a0..01` | guest | yes |
| `a0..02` | member | yes |
| `a0..03` | club_admin | yes |
| `a0..04` | platform_admin | yes |

### Permissions
One `(resource, action)` pair for each combination of:
- Resources: users, candidates, offers, approvals, onboarding, bookings, sites, reports, reporting, integrations, audit, vocabularies, search, roles, permissions
- Actions: read, create, update, delete

`platform_admin` receives all permissions.

### Users
| Username | Role |
|----------|------|
| `platform_admin` | platform_admin |
| `club_admin` | club_admin |
| `member` | member |

### Controlled Vocabularies
- Departments: engineering, sales, hr, finance
- Candidate sources: referral, job_board, direct
- Candidate tags: senior, junior, remote

### Office Sites
| Code | Name | Location |
|------|------|----------|
| HQ | Headquarters | New York |
| WEST | West Coast Office | San Francisco |
| REMOTE | Remote / Virtual | N/A |
