-- ============================================================
-- Migration 4: Booking transactional workflow
-- ============================================================

-- booking_status enum values 'pending_confirmation' and 'exception' are included
-- in the initial schema migration (migration 1) to avoid ALTER TYPE ADD VALUE
-- inside a transaction (which PostgreSQL forbids).

-- ============================================================
-- Inventory slots: time-bounded, capacity-limited orientation slots
-- ============================================================

CREATE TABLE booking_slots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES office_sites(id),
    slot_date DATE NOT NULL,
    start_time TIME NOT NULL,
    end_time TIME NOT NULL,
    capacity INT NOT NULL CHECK (capacity > 0),
    booked_count INT NOT NULL DEFAULT 0 CHECK (booked_count >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_slot_time_order CHECK (end_time > start_time),
    CONSTRAINT chk_booked_le_capacity CHECK (booked_count <= capacity),
    CONSTRAINT uq_slot_site_date_time UNIQUE (site_id, slot_date, start_time, end_time)
);

CREATE INDEX idx_booking_slots_site_date ON booking_slots(site_id, slot_date);
CREATE INDEX idx_booking_slots_available ON booking_slots(slot_date) WHERE booked_count < capacity;

CREATE TRIGGER trg_booking_slots_updated_at
    BEFORE UPDATE ON booking_slots
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- ============================================================
-- Expand booking_orders for transactional workflow
-- ============================================================

-- slot_id: FK to the inventory slot this booking holds
ALTER TABLE booking_orders
    ADD COLUMN slot_id UUID REFERENCES booking_slots(id);

-- Hold management
ALTER TABLE booking_orders
    ADD COLUMN hold_expires_at TIMESTAMPTZ;

-- Agreement confirmation evidence (not a bare boolean)
ALTER TABLE booking_orders
    ADD COLUMN agreement_signed_by VARCHAR(256),
    ADD COLUMN agreement_signed_at TIMESTAMPTZ,
    ADD COLUMN agreement_hash VARCHAR(128);

-- Breach tracking for late cancellations
ALTER TABLE booking_orders
    ADD COLUMN breach_reason TEXT,
    ADD COLUMN breach_reason_code VARCHAR(64);

-- Exception state details
ALTER TABLE booking_orders
    ADD COLUMN exception_detail TEXT;

-- Idempotency key reference (for deduplication)
ALTER TABLE booking_orders
    ADD COLUMN idempotency_key VARCHAR(256);

CREATE INDEX idx_booking_orders_hold_expires ON booking_orders(hold_expires_at)
    WHERE status = 'pending_confirmation';
CREATE INDEX idx_booking_orders_idempotency ON booking_orders(idempotency_key)
    WHERE idempotency_key IS NOT NULL;
CREATE INDEX idx_booking_orders_slot ON booking_orders(slot_id);

-- ============================================================
-- Booking restrictions: blocks that prevent confirmation
-- ============================================================

CREATE TABLE booking_restrictions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    candidate_id UUID NOT NULL REFERENCES candidates(id),
    restriction_type VARCHAR(64) NOT NULL,
    reason TEXT,
    is_active BOOLEAN NOT NULL DEFAULT true,
    expires_at TIMESTAMPTZ,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_booking_restrictions_candidate ON booking_restrictions(candidate_id)
    WHERE is_active = true;

CREATE TRIGGER trg_booking_restrictions_updated_at
    BEFORE UPDATE ON booking_restrictions
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
