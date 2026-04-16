#!/usr/bin/env bash
set -euo pipefail

echo "=== TalentFlow Test Runner ==="
echo ""

# ---------------------------------------------------------------------------
# Flag parsing
# ---------------------------------------------------------------------------
SKIP_INTEGRATION=false
FORCE_DOCKER=false
for arg in "$@"; do
  [[ "$arg" == "--skip-integration" ]] && SKIP_INTEGRATION=true
  [[ "$arg" == "--docker" ]] && FORCE_DOCKER=true
done

# ---------------------------------------------------------------------------
# Detect docker compose command (V2 plugin vs V1 standalone)
# ---------------------------------------------------------------------------
if docker compose version &>/dev/null 2>&1; then
    DC="docker compose"
elif command -v docker-compose &>/dev/null; then
    DC="docker-compose"
else
    echo "ERROR: Neither 'docker compose' (V2) nor 'docker-compose' (V1) was found."
    echo "Install Docker Desktop or docker-compose-plugin and try again."
    exit 1
fi

# ---------------------------------------------------------------------------
# Decide execution path:
#   local cargo  — when cargo is present AND database env vars are set
#                  AND --docker was not requested
#   docker       — everything else (no cargo, no DB env, or --docker flag)
# ---------------------------------------------------------------------------
USE_LOCAL=false
if command -v cargo &>/dev/null && [ "$FORCE_DOCKER" = "false" ]; then
    if [ -n "${DATABASE_URL:-}" ] && [ -n "${ENCRYPTION_KEY:-}" ]; then
        USE_LOCAL=true
    else
        echo "INFO: cargo found but DATABASE_URL/ENCRYPTION_KEY not set — using Docker path."
        echo "      (Pass --skip-integration or set env vars to run locally.)"
        echo ""
    fi
fi

# ---------------------------------------------------------------------------
# LOCAL PATH
# ---------------------------------------------------------------------------
if [ "$USE_LOCAL" = "true" ]; then
    echo "Using local cargo toolchain"
    echo ""

    echo "--- Checking formatting ---"
    cargo fmt -- --check

    echo ""
    echo "--- Running clippy ---"
    cargo clippy -- -D warnings

    echo ""
    echo "--- Running unit tests (no database required) ---"
    cargo test --test unit_tests

    echo ""
    echo "--- Running integration tests ---"
    if [ "$SKIP_INTEGRATION" = "true" ]; then
        echo "WARNING: --skip-integration passed — integration tests SKIPPED"
    else
        cargo test --test auth_tests
        cargo test --test business_tests
        cargo test --test booking_tests
        cargo test --test search_reporting_tests
        cargo test --test scheduler_integration_tests
    fi

# ---------------------------------------------------------------------------
# DOCKER PATH
# ---------------------------------------------------------------------------
else
    echo "Running tests inside Docker (DC: $DC)"
    echo ""

    # Ensure the DB is up
    $DC up -d db
    echo "Waiting for database to be healthy..."
    $DC exec db sh -c 'until pg_isready -U talentflow; do sleep 1; done' 2>/dev/null

    # Build test image
    echo ""
    echo "--- Building test image ---"
    docker build -f Dockerfile.test -t talentflow-test .
    echo ""

    # Generate a one-time encryption key for tests
    TEST_KEY=$(openssl rand -base64 32)

    # Detect the compose network name
    NETWORK=$($DC ps -q db | head -1 | xargs -I{} docker inspect {} \
        -f '{{range $k,$v := .NetworkSettings.Networks}}{{$k}}{{end}}' 2>/dev/null | head -1)

    # Create a fresh test database and seed it
    echo "--- Preparing test database ---"
    $DC exec db psql -U talentflow -c "DROP DATABASE IF EXISTS talentflow_test;" 2>/dev/null
    $DC exec db psql -U talentflow -c "CREATE DATABASE talentflow_test;" 2>/dev/null
    docker run --rm --network "$NETWORK" \
        -e DATABASE_URL="postgres://talentflow:talentflow_dev@db:5432/talentflow_test" \
        -e ENCRYPTION_KEY="$TEST_KEY" \
        -e STORAGE_PATH="/tmp/test_storage" \
        -e SEED_ADMIN_PASSWORD='Admin_Pa$$word1!' \
        -e SEED_CLUB_ADMIN_PASSWORD='ClubAdm1n!Passw0rd' \
        -e SEED_MEMBER_PASSWORD='Member!User1Passw0rd' \
        -w /app talentflow-test \
        cargo run --bin seed 2>&1 | tail -5
    echo ""

    # Run unit tests (no DB needed)
    echo "--- Running unit tests ---"
    docker run --rm -w /app talentflow-test \
        cargo test --test unit_tests 2>&1
    echo ""

    # Run integration tests one at a time against the test DB
    if [ "$SKIP_INTEGRATION" = "true" ]; then
        echo "WARNING: --skip-integration passed — integration tests SKIPPED"
    else
        echo "--- Running integration tests ---"
        for suite in auth_tests business_tests booking_tests search_reporting_tests scheduler_integration_tests; do
            echo "  >> $suite"
            # Reset lockouts between suites to prevent cascading failures
            $DC exec db psql -U talentflow -d talentflow_test \
                -c "UPDATE users SET failed_login_count=0, locked_until=NULL, account_status='active';" 2>/dev/null
            docker run --rm \
                --network "$NETWORK" \
                -e DATABASE_URL="postgres://talentflow:talentflow_dev@db:5432/talentflow_test" \
                -e ENCRYPTION_KEY="$TEST_KEY" \
                -e STORAGE_PATH="/tmp/test_storage" \
                -w /app \
                talentflow-test \
                cargo test --test "$suite" 2>&1
            echo ""
        done
    fi
fi

echo "=== All checks passed ==="
