-- Add explicit, queryable compensation columns to the offers table.
--
-- Migration plan (three-phase):
--
--   Phase 1 (this migration):
--     - Add nullable structured columns alongside the existing
--       compensation_encrypted blob.
--     - Application dual-writes both representations on create/update.
--
--   Phase 2 (backfill):
--     - Run `cargo run --bin backfill_compensation` to decrypt existing
--       blobs and populate the new columns for historical rows.
--     - Validate: SELECT count(*) FROM offers
--                 WHERE compensation_encrypted IS NOT NULL
--                   AND salary_cents IS NULL;  -- should be 0
--
--   Phase 3 (future migration, after backfill is verified):
--     - Drop compensation_encrypted column.
--     - Remove encryption/decryption code paths.
--     - NOT included in this migration — requires its own review cycle.

ALTER TABLE offers
    ADD COLUMN salary_cents      BIGINT,
    ADD COLUMN bonus_target_pct  DOUBLE PRECISION,
    ADD COLUMN equity_units      INTEGER,
    ADD COLUMN pto_days          SMALLINT,
    ADD COLUMN k401_match_pct    DOUBLE PRECISION,
    ADD COLUMN currency          VARCHAR(3) NOT NULL DEFAULT 'USD';

COMMENT ON COLUMN offers.salary_cents     IS 'Annual base salary in cents (e.g. 100000_00 = $100,000). Populated alongside compensation_encrypted during transition.';
COMMENT ON COLUMN offers.currency         IS 'ISO 4217 currency code. Defaults to USD for backward compatibility.';
