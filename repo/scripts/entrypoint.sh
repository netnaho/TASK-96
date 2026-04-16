#!/usr/bin/env bash
set -euo pipefail

if [ "${RUN_SEED:-false}" = "true" ]; then
    echo "Running seed..."
    ./seed
fi

exec ./talentflow
