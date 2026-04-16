FROM rust:1.88-bookworm AS builder

RUN apt-get update && apt-get install -y libpq-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependency build — compile deps only, then remove stub artifacts
# so that the real build in the next step is forced to recompile application code.
COPY Cargo.toml Cargo.lock* ./
# Create minimal stubs for every target Cargo.toml references so that the
# dependency-caching layer compiles successfully and is reused on rebuilds.
RUN mkdir -p src/bin unit_tests API_tests e2e_tests && \
    echo 'fn main() {}' > src/bin/server.rs && \
    echo 'fn main() {}' > src/bin/seed.rs && \
    echo 'fn main() {}' > src/bin/backfill_compensation.rs && \
    printf '' > src/lib.rs && \
    printf '' > unit_tests/mod.rs && \
    printf '' > API_tests/auth_tests.rs && \
    printf '' > API_tests/business_tests.rs && \
    printf '' > API_tests/booking_tests.rs && \
    printf '' > API_tests/search_reporting_tests.rs && \
    printf '' > API_tests/scheduler_integration_tests.rs && \
    printf '' > e2e_tests/http_tests.rs && \
    cargo build --release 2>/dev/null || true && \
    rm -rf src unit_tests API_tests e2e_tests && \
    rm -f target/release/talentflow target/release/seed \
          target/release/backfill_compensation target/release/libtalentflow* && \
    rm -rf target/release/.fingerprint/talentflow-* \
           target/release/deps/talentflow-* target/release/deps/libtalentflow-*

COPY . .
RUN cargo build --release

# -----------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends libpq5 ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1000 app && \
    useradd --uid 1000 --gid app --create-home app

WORKDIR /app

COPY --from=builder /app/target/release/talentflow ./talentflow
COPY --from=builder /app/target/release/seed ./seed
COPY --from=builder /app/migrations ./migrations
COPY scripts/entrypoint.sh ./entrypoint.sh

RUN mkdir -p /app/storage && chown -R app:app /app

USER app

EXPOSE 8080

ENTRYPOINT ["./entrypoint.sh"]
