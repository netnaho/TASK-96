# API Integration Tests

Integration tests in this directory exercise the full HTTP stack against a running
application instance with a real PostgreSQL database.

## Test Categories (planned)

- **Auth flow**: login, session validation, logout, lockout
- **CRUD lifecycle**: create/read/update for each resource
- **State transitions**: offer approval workflow, onboarding completion
- **Authorization**: permission-based access control per role
- **Error handling**: validation errors, not found, conflicts
- **Idempotency**: duplicate request replay
- **Pagination**: page/per_page parameters
- **Search**: full-text and filtered queries
- **Audit**: event creation and read-only enforcement

## Running

```bash
# Requires a running PostgreSQL instance (use docker compose)
cargo test --test '*'
```
