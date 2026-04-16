-- Reverse of initial schema migration

DROP TRIGGER IF EXISTS trg_integration_sync_state_updated_at ON integration_sync_state;
DROP TRIGGER IF EXISTS trg_integration_connectors_updated_at ON integration_connectors;
DROP TRIGGER IF EXISTS trg_reporting_subscriptions_updated_at ON reporting_subscriptions;
DROP TRIGGER IF EXISTS trg_booking_orders_updated_at ON booking_orders;
DROP TRIGGER IF EXISTS trg_onboarding_items_updated_at ON onboarding_items;
DROP TRIGGER IF EXISTS trg_onboarding_checklists_updated_at ON onboarding_checklists;
DROP TRIGGER IF EXISTS trg_offers_updated_at ON offers;
DROP TRIGGER IF EXISTS trg_candidates_updated_at ON candidates;
DROP TRIGGER IF EXISTS trg_users_updated_at ON users;

DROP FUNCTION IF EXISTS set_updated_at();

DROP TRIGGER IF EXISTS trg_audit_no_delete ON audit_events;
DROP TRIGGER IF EXISTS trg_audit_no_update ON audit_events;
DROP FUNCTION IF EXISTS prevent_audit_mutation();

DROP TABLE IF EXISTS audit_events;
DROP TABLE IF EXISTS idempotency_keys;
DROP TABLE IF EXISTS integration_sync_state;
DROP TABLE IF EXISTS integration_connectors;
DROP TABLE IF EXISTS reporting_alerts;
DROP TABLE IF EXISTS dashboard_versions;
DROP TABLE IF EXISTS reporting_subscriptions;
DROP TABLE IF EXISTS historical_queries;
DROP TABLE IF EXISTS controlled_vocabularies;
DROP TABLE IF EXISTS booking_orders;
DROP TABLE IF EXISTS office_sites;
DROP TABLE IF EXISTS onboarding_items;
DROP TABLE IF EXISTS onboarding_checklists;
DROP TABLE IF EXISTS approval_steps;
DROP TABLE IF EXISTS offers;
DROP TABLE IF EXISTS candidates;
DROP TABLE IF EXISTS user_roles;
DROP TABLE IF EXISTS role_permissions;
DROP TABLE IF EXISTS permissions;
DROP TABLE IF EXISTS roles;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS users;
