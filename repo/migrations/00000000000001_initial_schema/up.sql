-- TalentFlow: Initial Schema Migration
-- This migration establishes all core tables, enums, indexes, and constraints.

-- ============================================================
-- Note: Status columns use VARCHAR(32) instead of PostgreSQL enums.
-- This avoids ALTER TYPE ADD VALUE issues in transactional migrations
-- and aligns with Diesel's schema mapping (Varchar for all status fields).
-- Valid values are enforced at the application layer.
-- ============================================================

-- ============================================================
-- Identity & Access Control
-- ============================================================

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(128) NOT NULL,
    email VARCHAR(256) NOT NULL,
    password_hash VARCHAR(512) NOT NULL,
    display_name VARCHAR(256) NOT NULL,
    account_status VARCHAR(32) NOT NULL DEFAULT 'active',
    failed_login_count INT NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    last_login_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_users_username UNIQUE (username),
    CONSTRAINT uq_users_email UNIQUE (email)
);

CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(512) NOT NULL,
    device_fingerprint VARCHAR(512),
    ip_address VARCHAR(45),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_sessions_token_hash UNIQUE (token_hash)
);

CREATE INDEX idx_sessions_user_id ON sessions(user_id);
CREATE INDEX idx_sessions_expires_at ON sessions(expires_at);

CREATE TABLE roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(64) NOT NULL,
    description TEXT,
    is_system_role BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_roles_name UNIQUE (name)
);

CREATE TABLE permissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource VARCHAR(128) NOT NULL,
    action VARCHAR(64) NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_permissions_resource_action UNIQUE (resource, action)
);

CREATE TABLE role_permissions (
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    scope_type VARCHAR(64),
    scope_id UUID,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    granted_by UUID REFERENCES users(id)
);

-- Unique constraint on (user_id, role_id, scope_type, scope_id) allowing NULLs
-- via a unique index with COALESCE to treat NULL scope as a single unscoped assignment.
CREATE UNIQUE INDEX uq_user_roles_assignment
    ON user_roles (user_id, role_id, COALESCE(scope_type, ''), COALESCE(scope_id, '00000000-0000-0000-0000-000000000000'));

CREATE INDEX idx_user_roles_user_id ON user_roles(user_id);
CREATE INDEX idx_user_roles_scope ON user_roles(scope_type, scope_id);

-- ============================================================
-- Candidate Profiles
-- ============================================================

CREATE TABLE candidates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    first_name VARCHAR(128) NOT NULL,
    last_name VARCHAR(128) NOT NULL,
    email VARCHAR(256) NOT NULL,
    phone_encrypted BYTEA,
    ssn_last4_encrypted BYTEA,
    resume_storage_key VARCHAR(512),
    source VARCHAR(128),
    tags TEXT[] NOT NULL DEFAULT '{}',
    notes TEXT,
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_candidates_email UNIQUE (email)
);

CREATE INDEX idx_candidates_name ON candidates(last_name, first_name);
CREATE INDEX idx_candidates_tags ON candidates USING gin(tags);

-- ============================================================
-- Offers & Approvals
-- ============================================================

CREATE TABLE offers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    candidate_id UUID NOT NULL REFERENCES candidates(id),
    title VARCHAR(256) NOT NULL,
    department VARCHAR(128),
    compensation_encrypted BYTEA,
    start_date DATE,
    status VARCHAR(32) NOT NULL DEFAULT 'draft',
    expires_at TIMESTAMPTZ,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_offers_candidate_id ON offers(candidate_id);
CREATE INDEX idx_offers_status ON offers(status);

CREATE TABLE approval_steps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    offer_id UUID NOT NULL REFERENCES offers(id) ON DELETE CASCADE,
    step_order INT NOT NULL,
    approver_id UUID NOT NULL REFERENCES users(id),
    decision VARCHAR(32) NOT NULL DEFAULT 'pending',
    decided_at TIMESTAMPTZ,
    comments TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_approval_step_order UNIQUE (offer_id, step_order)
);

CREATE INDEX idx_approval_steps_offer_id ON approval_steps(offer_id);
CREATE INDEX idx_approval_steps_approver ON approval_steps(approver_id);

-- ============================================================
-- Onboarding
-- ============================================================

CREATE TABLE onboarding_checklists (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    offer_id UUID NOT NULL REFERENCES offers(id),
    candidate_id UUID NOT NULL REFERENCES candidates(id),
    assigned_to UUID REFERENCES users(id),
    due_date DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_onboarding_checklists_offer ON onboarding_checklists(offer_id);

CREATE TABLE onboarding_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    checklist_id UUID NOT NULL REFERENCES onboarding_checklists(id) ON DELETE CASCADE,
    title VARCHAR(256) NOT NULL,
    description TEXT,
    item_order INT NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'not_started',
    requires_upload BOOLEAN NOT NULL DEFAULT false,
    upload_storage_key VARCHAR(512),
    health_attestation_encrypted BYTEA,
    completed_at TIMESTAMPTZ,
    completed_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_onboarding_items_checklist ON onboarding_items(checklist_id);

-- ============================================================
-- Bookings & Orders
-- ============================================================

CREATE TABLE office_sites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code VARCHAR(32) NOT NULL,
    name VARCHAR(256) NOT NULL,
    address TEXT,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    timezone VARCHAR(64) NOT NULL DEFAULT 'UTC',
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_office_sites_code UNIQUE (code)
);

CREATE TABLE booking_orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    candidate_id UUID NOT NULL REFERENCES candidates(id),
    site_id UUID NOT NULL REFERENCES office_sites(id),
    status VARCHAR(32) NOT NULL DEFAULT 'draft',
    scheduled_date DATE NOT NULL,
    scheduled_time_start TIME,
    scheduled_time_end TIME,
    notes TEXT,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_booking_orders_candidate ON booking_orders(candidate_id);
CREATE INDEX idx_booking_orders_site_date ON booking_orders(site_id, scheduled_date);
CREATE INDEX idx_booking_orders_status ON booking_orders(status);

-- ============================================================
-- Controlled Vocabularies & Tags
-- ============================================================

CREATE TABLE controlled_vocabularies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    category VARCHAR(128) NOT NULL,
    value VARCHAR(256) NOT NULL,
    label VARCHAR(256) NOT NULL,
    sort_order INT NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_vocab_category_value UNIQUE (category, value)
);

CREATE INDEX idx_controlled_vocab_category ON controlled_vocabularies(category);

-- ============================================================
-- Search & Historical Queries
-- ============================================================

CREATE TABLE historical_queries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    query_text TEXT NOT NULL,
    filters JSONB NOT NULL DEFAULT '{}',
    result_count INT,
    executed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_historical_queries_user ON historical_queries(user_id);
CREATE INDEX idx_historical_queries_executed ON historical_queries(executed_at);

-- ============================================================
-- Reporting
-- ============================================================

CREATE TABLE reporting_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    report_type VARCHAR(128) NOT NULL,
    parameters JSONB NOT NULL DEFAULT '{}',
    cron_expression VARCHAR(64),
    is_active BOOLEAN NOT NULL DEFAULT true,
    last_run_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_reporting_subs_user ON reporting_subscriptions(user_id);

CREATE TABLE dashboard_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    dashboard_key VARCHAR(128) NOT NULL,
    version INT NOT NULL,
    layout JSONB NOT NULL,
    published_by UUID NOT NULL REFERENCES users(id),
    published_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_dashboard_version UNIQUE (dashboard_key, version)
);

CREATE TABLE reporting_alerts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscription_id UUID NOT NULL REFERENCES reporting_subscriptions(id) ON DELETE CASCADE,
    severity VARCHAR(32) NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    acknowledged BOOLEAN NOT NULL DEFAULT false,
    acknowledged_by UUID REFERENCES users(id),
    acknowledged_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_reporting_alerts_sub ON reporting_alerts(subscription_id);
CREATE INDEX idx_reporting_alerts_unack ON reporting_alerts(acknowledged) WHERE acknowledged = false;

-- ============================================================
-- Integrations
-- ============================================================

CREATE TABLE integration_connectors (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(128) NOT NULL,
    connector_type VARCHAR(32) NOT NULL,
    base_url VARCHAR(512),
    auth_config_encrypted BYTEA,
    is_enabled BOOLEAN NOT NULL DEFAULT false,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_connector_name UNIQUE (name)
);

CREATE TABLE integration_sync_state (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    connector_id UUID NOT NULL REFERENCES integration_connectors(id) ON DELETE CASCADE,
    entity_type VARCHAR(128) NOT NULL,
    last_sync_at TIMESTAMPTZ,
    last_sync_cursor VARCHAR(512),
    status VARCHAR(32) NOT NULL DEFAULT 'idle',
    error_message TEXT,
    record_count INT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_sync_state_connector_entity UNIQUE (connector_id, entity_type)
);

-- ============================================================
-- Idempotency
-- ============================================================

CREATE TABLE idempotency_keys (
    key VARCHAR(256) PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    request_path VARCHAR(512) NOT NULL,
    request_hash VARCHAR(128) NOT NULL,
    response_status INT NOT NULL,
    response_body JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_idempotency_expires ON idempotency_keys(expires_at);

-- ============================================================
-- Audit Events (append-only)
-- ============================================================

CREATE TABLE audit_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id UUID REFERENCES users(id),
    actor_ip VARCHAR(45),
    action VARCHAR(128) NOT NULL,
    resource_type VARCHAR(128) NOT NULL,
    resource_id UUID,
    old_value JSONB,
    new_value JSONB,
    metadata JSONB NOT NULL DEFAULT '{}',
    correlation_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_events_actor ON audit_events(actor_id);
CREATE INDEX idx_audit_events_resource ON audit_events(resource_type, resource_id);
CREATE INDEX idx_audit_events_action ON audit_events(action);
CREATE INDEX idx_audit_events_created ON audit_events(created_at);
CREATE INDEX idx_audit_events_correlation ON audit_events(correlation_id);

-- Prevent UPDATE/DELETE on audit_events
CREATE OR REPLACE FUNCTION prevent_audit_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is append-only: % operations are not permitted', TG_OP;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_audit_no_update
    BEFORE UPDATE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION prevent_audit_mutation();

CREATE TRIGGER trg_audit_no_delete
    BEFORE DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION prevent_audit_mutation();

-- ============================================================
-- Updated-at trigger function (reusable)
-- ============================================================

CREATE OR REPLACE FUNCTION set_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_users_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_candidates_updated_at BEFORE UPDATE ON candidates FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_offers_updated_at BEFORE UPDATE ON offers FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_onboarding_checklists_updated_at BEFORE UPDATE ON onboarding_checklists FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_onboarding_items_updated_at BEFORE UPDATE ON onboarding_items FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_booking_orders_updated_at BEFORE UPDATE ON booking_orders FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_reporting_subscriptions_updated_at BEFORE UPDATE ON reporting_subscriptions FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_integration_connectors_updated_at BEFORE UPDATE ON integration_connectors FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_integration_sync_state_updated_at BEFORE UPDATE ON integration_sync_state FOR EACH ROW EXECUTE FUNCTION set_updated_at();
