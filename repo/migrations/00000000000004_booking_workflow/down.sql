DROP TABLE IF EXISTS booking_restrictions;

DROP INDEX IF EXISTS idx_booking_orders_slot;
DROP INDEX IF EXISTS idx_booking_orders_idempotency;
DROP INDEX IF EXISTS idx_booking_orders_hold_expires;

ALTER TABLE booking_orders
    DROP COLUMN IF EXISTS idempotency_key,
    DROP COLUMN IF EXISTS exception_detail,
    DROP COLUMN IF EXISTS breach_reason_code,
    DROP COLUMN IF EXISTS breach_reason,
    DROP COLUMN IF EXISTS agreement_hash,
    DROP COLUMN IF EXISTS agreement_signed_at,
    DROP COLUMN IF EXISTS agreement_signed_by,
    DROP COLUMN IF EXISTS hold_expires_at,
    DROP COLUMN IF EXISTS slot_id;

DROP TABLE IF EXISTS booking_slots;

-- NOTE: PostgreSQL does not support DROP VALUE from an enum.
-- The added enum values (pending_confirmation, exception) remain but are unused after rollback.
