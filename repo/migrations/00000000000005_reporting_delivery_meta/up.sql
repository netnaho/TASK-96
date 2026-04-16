-- Additive: nullable delivery metadata column on reporting_alerts.
-- Stores best-effort delivery attempt outcome (delivered/skipped/error)
-- for each alert produced by the daily snapshot job.
ALTER TABLE reporting_alerts
    ADD COLUMN IF NOT EXISTS delivery_meta JSONB NULL;
