#!/usr/bin/env bash
set -euo pipefail

# Reset the development database: drop, recreate, migrate, and seed.
# Requires DATABASE_URL to be set or a running docker compose db service.

DB_URL="${DATABASE_URL:-postgres://talentflow:talentflow_dev@localhost:5432/talentflow}"

echo "Dropping and recreating database..."
psql "$DB_URL" -c "SELECT 1" >/dev/null 2>&1 || {
    echo "Cannot connect to database. Is PostgreSQL running?"
    exit 1
}

diesel database reset --database-url "$DB_URL"

echo "Running seed..."
cargo run --bin seed

echo "Database reset complete."
