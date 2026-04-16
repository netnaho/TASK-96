-- Track last activity for idle-timeout enforcement (8-hour idle window).
-- NULL means no activity recorded yet; treat as created_at for idle calculation.
ALTER TABLE sessions
    ADD COLUMN last_activity_at TIMESTAMPTZ;
