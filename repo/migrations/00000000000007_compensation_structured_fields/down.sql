ALTER TABLE offers
    DROP COLUMN IF EXISTS salary_cents,
    DROP COLUMN IF EXISTS bonus_target_pct,
    DROP COLUMN IF EXISTS equity_units,
    DROP COLUMN IF EXISTS pto_days,
    DROP COLUMN IF EXISTS k401_match_pct,
    DROP COLUMN IF EXISTS currency;
