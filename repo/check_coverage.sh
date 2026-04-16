#!/usr/bin/env bash
# Optional coverage report.  Requires cargo-tarpaulin to be installed:
#   cargo install cargo-tarpaulin
#
# Full coverage (unit + API integration):
#   DATABASE_URL=... ENCRYPTION_KEY=... ./check_coverage.sh
#
# Unit-only coverage (no database needed):
#   ./check_coverage.sh
set -euo pipefail

echo "=== TalentFlow Coverage Check ==="
echo ""

if ! command -v cargo-tarpaulin &>/dev/null && ! cargo tarpaulin --version &>/dev/null 2>&1; then
    echo "cargo-tarpaulin is not installed. Install it with:"
    echo "  cargo install cargo-tarpaulin"
    exit 1
fi

if [ -n "${DATABASE_URL:-}" ] && [ -n "${ENCRYPTION_KEY:-}" ]; then
    echo "Running full coverage (unit + API integration tests)..."
    cargo tarpaulin \
        --tests \
        --engine llvm \
        --out Html \
        --output-dir target/tarpaulin \
        --fail-under 90 \
        --timeout 300 \
        --test-threads 1 \
        2>&1
    echo ""
    echo "Coverage report: target/tarpaulin/tarpaulin-report.html"
else
    echo "No DATABASE_URL/ENCRYPTION_KEY — running unit-test-only coverage..."
    echo "(Set both env vars to include API integration test coverage.)"
    echo ""
    cargo tarpaulin \
        --test unit_tests \
        --engine llvm \
        --out Html \
        --output-dir target/tarpaulin \
        --fail-under 90 \
        --timeout 120 \
        2>&1
    echo ""
    echo "Coverage report: target/tarpaulin/tarpaulin-report.html"
fi

echo ""
echo "=== Coverage check complete ==="
