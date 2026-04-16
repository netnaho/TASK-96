-- Additive migration: add optional home-location coordinates to candidates.
-- Used by the search service to compute Haversine distance from a reference
-- office site when `site_code` is provided.
-- Nullable with no default — existing rows remain NULL (no distance basis).
ALTER TABLE candidates ADD COLUMN latitude  DOUBLE PRECISION;
ALTER TABLE candidates ADD COLUMN longitude DOUBLE PRECISION;
