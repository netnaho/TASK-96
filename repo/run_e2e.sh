#!/usr/bin/env bash
set -euo pipefail

echo "=== TalentFlow E2E Test Runner ==="
echo ""

# ---------------------------------------------------------------------------
# Cleanup: stop the app container on exit (success or failure)
# ---------------------------------------------------------------------------
cleanup() {
    echo ""
    echo "--- Stopping app container ---"
    docker compose stop app
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Start the database and application
# ---------------------------------------------------------------------------
echo "--- Starting db and app ---"
docker compose up -d db app

# ---------------------------------------------------------------------------
# Wait for the app to become healthy
# ---------------------------------------------------------------------------
echo "--- Waiting for app to be ready ---"
MAX_WAIT=60
ELAPSED=0
until curl -sf http://localhost:8080/api/v1/health >/dev/null 2>&1; do
    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: app did not become healthy within ${MAX_WAIT}s"
        exit 1
    fi
    sleep 2
    ELAPSED=$((ELAPSED + 2))
done
echo "App is healthy (waited ${ELAPSED}s)"
echo ""

# ---------------------------------------------------------------------------
# Run the E2E test suite
# ---------------------------------------------------------------------------
echo "--- Running E2E tests ---"
E2E_BASE_URL=http://localhost:8080 cargo test --test e2e_tests

echo ""
echo "=== E2E tests passed ==="
