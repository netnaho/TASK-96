/// Diesel ORM structs: one `Db*` (Queryable) and one `New*` (Insertable) per table.
/// These are infrastructure-only types — domain models live in src/domain/*.
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::schema::{
    approval_steps, audit_events, booking_orders, booking_restrictions, booking_slots, candidates,
    controlled_vocabularies, dashboard_versions, historical_queries, idempotency_keys,
    integration_connectors, integration_sync_state, offers, office_sites, onboarding_checklists,
    onboarding_items, permissions, reporting_alerts, reporting_subscriptions, role_permissions,
    roles, sessions, user_roles, users,
};

// ============================================================
// users
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbUser {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub account_status: String,
    pub failed_login_count: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = users)]
pub struct NewDbUser {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub account_status: String,
}

// ============================================================
// sessions
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub device_fingerprint: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: Option<DateTime<Utc>>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = sessions)]
pub struct NewDbSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub device_fingerprint: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub last_activity_at: Option<DateTime<Utc>>,
}

// ============================================================
// roles
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = roles)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbRole {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system_role: bool,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// permissions
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = permissions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbPermission {
    pub id: Uuid,
    pub resource: String,
    pub action: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// role_permissions
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = role_permissions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbRolePermission {
    pub role_id: Uuid,
    pub permission_id: Uuid,
}

// ============================================================
// user_roles
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = user_roles)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbUserRole {
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub scope_type: Option<String>,
    pub scope_id: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
    pub granted_by: Option<Uuid>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = user_roles)]
pub struct NewDbUserRole {
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub scope_type: Option<String>,
    pub scope_id: Option<Uuid>,
    pub granted_by: Option<Uuid>,
}

// ============================================================
// candidates
// ============================================================

#[derive(Queryable, Selectable, QueryableByName, Debug, Clone)]
#[diesel(table_name = candidates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbCandidate {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone_encrypted: Option<Vec<u8>>,
    pub ssn_last4_encrypted: Option<Vec<u8>>,
    pub resume_storage_key: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Home-location coordinates added in migration 00000000000006.
    /// NULL for candidates created before the migration or via the API
    /// (latitude/longitude are not exposed in the create/update request body).
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = candidates)]
pub struct NewDbCandidate {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone_encrypted: Option<Vec<u8>>,
    pub ssn_last4_encrypted: Option<Vec<u8>>,
    pub resume_storage_key: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

// ============================================================
// offers
// ============================================================

#[derive(Queryable, Selectable, QueryableByName, Debug, Clone)]
#[diesel(table_name = offers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbOffer {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub title: String,
    pub department: Option<String>,
    pub compensation_encrypted: Option<Vec<u8>>,
    pub start_date: Option<NaiveDate>,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub template_id: Option<Uuid>,
    pub clause_version: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub salary_cents: Option<i64>,
    pub bonus_target_pct: Option<f64>,
    pub equity_units: Option<i32>,
    pub pto_days: Option<i16>,
    pub k401_match_pct: Option<f64>,
    pub currency: String,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = offers)]
pub struct NewDbOffer {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub title: String,
    pub department: Option<String>,
    pub compensation_encrypted: Option<Vec<u8>>,
    pub start_date: Option<NaiveDate>,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub template_id: Option<Uuid>,
    pub clause_version: Option<String>,
    pub created_by: Uuid,
    pub salary_cents: Option<i64>,
    pub bonus_target_pct: Option<f64>,
    pub equity_units: Option<i32>,
    pub pto_days: Option<i16>,
    pub k401_match_pct: Option<f64>,
    pub currency: String,
}

// ============================================================
// approval_steps
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = approval_steps)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbApprovalStep {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub step_order: i32,
    pub approver_id: Uuid,
    pub decision: String,
    pub decided_at: Option<DateTime<Utc>>,
    pub comments: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = approval_steps)]
pub struct NewDbApprovalStep {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub step_order: i32,
    pub approver_id: Uuid,
    pub decision: String,
}

// ============================================================
// onboarding_checklists
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = onboarding_checklists)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbOnboardingChecklist {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub candidate_id: Uuid,
    pub assigned_to: Option<Uuid>,
    pub due_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = onboarding_checklists)]
pub struct NewDbOnboardingChecklist {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub candidate_id: Uuid,
    pub assigned_to: Option<Uuid>,
    pub due_date: Option<NaiveDate>,
}

// ============================================================
// onboarding_items
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = onboarding_items)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbOnboardingItem {
    pub id: Uuid,
    pub checklist_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub item_order: i32,
    pub status: String,
    pub requires_upload: bool,
    pub upload_storage_key: Option<String>,
    pub health_attestation_encrypted: Option<Vec<u8>>,
    pub required: bool,
    pub item_due_date: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub completed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = onboarding_items)]
pub struct NewDbOnboardingItem {
    pub id: Uuid,
    pub checklist_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub item_order: i32,
    pub status: String,
    pub requires_upload: bool,
    pub health_attestation_encrypted: Option<Vec<u8>>,
    pub required: bool,
    pub item_due_date: Option<NaiveDate>,
}

// ============================================================
// office_sites
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = office_sites)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbOfficeSite {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub address: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// booking_slots
// ============================================================

#[derive(Queryable, Selectable, QueryableByName, Debug, Clone)]
#[diesel(table_name = booking_slots)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbBookingSlot {
    pub id: Uuid,
    pub site_id: Uuid,
    pub slot_date: NaiveDate,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub capacity: i32,
    pub booked_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = booking_slots)]
pub struct NewDbBookingSlot {
    pub id: Uuid,
    pub site_id: Uuid,
    pub slot_date: NaiveDate,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub capacity: i32,
}

// ============================================================
// booking_orders (expanded)
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = booking_orders)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbBookingOrder {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub site_id: Uuid,
    pub status: String,
    pub scheduled_date: NaiveDate,
    pub scheduled_time_start: Option<NaiveTime>,
    pub scheduled_time_end: Option<NaiveTime>,
    pub notes: Option<String>,
    pub slot_id: Option<Uuid>,
    pub hold_expires_at: Option<DateTime<Utc>>,
    pub agreement_signed_by: Option<String>,
    pub agreement_signed_at: Option<DateTime<Utc>>,
    pub agreement_hash: Option<String>,
    pub breach_reason: Option<String>,
    pub breach_reason_code: Option<String>,
    pub exception_detail: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = booking_orders)]
pub struct NewDbBookingOrder {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub site_id: Uuid,
    pub status: String,
    pub scheduled_date: NaiveDate,
    pub scheduled_time_start: Option<NaiveTime>,
    pub scheduled_time_end: Option<NaiveTime>,
    pub notes: Option<String>,
    pub slot_id: Option<Uuid>,
    pub hold_expires_at: Option<DateTime<Utc>>,
    pub idempotency_key: Option<String>,
    pub created_by: Uuid,
}

// ============================================================
// booking_restrictions
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = booking_restrictions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbBookingRestriction {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub restriction_type: String,
    pub reason: Option<String>,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = booking_restrictions)]
pub struct NewDbBookingRestriction {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub restriction_type: String,
    pub reason: Option<String>,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
}

// ============================================================
// controlled_vocabularies
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = controlled_vocabularies)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbControlledVocabulary {
    pub id: Uuid,
    pub category: String,
    pub value: String,
    pub label: String,
    pub sort_order: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = controlled_vocabularies)]
pub struct NewDbControlledVocabulary {
    pub id: Uuid,
    pub category: String,
    pub value: String,
    pub label: String,
    pub sort_order: i32,
    pub is_active: bool,
}

// ============================================================
// historical_queries
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = historical_queries)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbHistoricalQuery {
    pub id: Uuid,
    pub user_id: Uuid,
    pub query_text: String,
    pub filters: serde_json::Value,
    pub result_count: Option<i32>,
    pub executed_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = historical_queries)]
pub struct NewDbHistoricalQuery {
    pub id: Uuid,
    pub user_id: Uuid,
    pub query_text: String,
    pub filters: serde_json::Value,
    pub result_count: Option<i32>,
    pub executed_at: DateTime<Utc>,
}

// ============================================================
// reporting_subscriptions
// ============================================================

#[derive(Queryable, Selectable, QueryableByName, Debug, Clone)]
#[diesel(table_name = reporting_subscriptions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbReportingSubscription {
    pub id: Uuid,
    pub user_id: Uuid,
    pub report_type: String,
    pub parameters: serde_json::Value,
    pub cron_expression: Option<String>,
    pub is_active: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = reporting_subscriptions)]
pub struct NewDbReportingSubscription {
    pub id: Uuid,
    pub user_id: Uuid,
    pub report_type: String,
    pub parameters: serde_json::Value,
    pub cron_expression: Option<String>,
    pub is_active: bool,
}

// ============================================================
// dashboard_versions
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = dashboard_versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbDashboardVersion {
    pub id: Uuid,
    pub dashboard_key: String,
    pub version: i32,
    pub layout: serde_json::Value,
    pub published_by: Uuid,
    pub published_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = dashboard_versions)]
pub struct NewDbDashboardVersion {
    pub id: Uuid,
    pub dashboard_key: String,
    pub version: i32,
    pub layout: serde_json::Value,
    pub published_by: Uuid,
    pub published_at: DateTime<Utc>,
}

// ============================================================
// reporting_alerts
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = reporting_alerts)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbReportingAlert {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub severity: String,
    pub message: String,
    pub acknowledged: bool,
    pub acknowledged_by: Option<Uuid>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Best-effort delivery attempt metadata set by the snapshot job.
    /// `null` for alerts created before migration 5 or when delivery is disabled.
    pub delivery_meta: Option<serde_json::Value>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = reporting_alerts)]
pub struct NewDbReportingAlert {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub severity: String,
    pub message: String,
    pub acknowledged: bool,
    /// Delivery attempt outcome written by the snapshot job; `None` skips the column.
    pub delivery_meta: Option<serde_json::Value>,
}

// ============================================================
// integration_connectors
// ============================================================

#[derive(Queryable, Selectable, QueryableByName, Debug, Clone)]
#[diesel(table_name = integration_connectors)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbIntegrationConnector {
    pub id: Uuid,
    pub name: String,
    pub connector_type: String,
    pub base_url: Option<String>,
    pub auth_config_encrypted: Option<Vec<u8>>,
    pub is_enabled: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = integration_connectors)]
pub struct NewDbIntegrationConnector {
    pub id: Uuid,
    pub name: String,
    pub connector_type: String,
    pub base_url: Option<String>,
    pub auth_config_encrypted: Option<Vec<u8>>,
    pub is_enabled: bool,
    pub created_by: Uuid,
}

// ============================================================
// integration_sync_state
// ============================================================

#[derive(Queryable, Selectable, QueryableByName, Debug, Clone)]
#[diesel(table_name = integration_sync_state)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbIntegrationSyncState {
    pub id: Uuid,
    pub connector_id: Uuid,
    pub entity_type: String,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_sync_cursor: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub record_count: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = integration_sync_state)]
pub struct NewDbIntegrationSyncState {
    pub id: Uuid,
    pub connector_id: Uuid,
    pub entity_type: String,
    pub status: String,
    pub record_count: i32,
    pub updated_at: DateTime<Utc>,
}

// ============================================================
// audit_events
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = audit_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbAuditEvent {
    pub id: Uuid,
    pub actor_id: Option<Uuid>,
    pub actor_ip: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub metadata: serde_json::Value,
    pub correlation_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// idempotency_keys
// ============================================================

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = idempotency_keys)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbIdempotencyKey {
    pub key: String,
    pub user_id: Uuid,
    pub request_path: String,
    pub request_hash: String,
    pub response_status: i32,
    pub response_body: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = idempotency_keys)]
pub struct NewDbIdempotencyKey {
    pub key: String,
    pub user_id: Uuid,
    pub request_path: String,
    pub request_hash: String,
    pub response_status: i32,
    pub response_body: Option<serde_json::Value>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = audit_events)]
pub struct NewDbAuditEvent {
    pub id: Uuid,
    pub actor_id: Option<Uuid>,
    pub actor_ip: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub metadata: serde_json::Value,
    pub correlation_id: Option<Uuid>,
}
