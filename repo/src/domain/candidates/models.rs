use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Candidate profile domain model.
/// Encrypted fields (phone, ssn_last4) are stored as opaque bytes in the DB
/// and only decrypted when explicitly requested by an authorized caller.
#[derive(Debug, Clone)]
pub struct Candidate {
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
    /// Used for club_admin scope enforcement: a club_admin scoped to an organization
    /// may only access candidates where organization_id matches their scope_id.
    pub organization_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Masked view of a candidate for list responses.
/// Sensitive fields (phone, ssn_last4) are omitted entirely.
#[derive(Debug, Serialize)]
pub struct CandidateSummary {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub organization_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Full candidate detail response, including optionally revealed sensitive fields.
#[derive(Debug, Serialize)]
pub struct CandidateDetail {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    /// Present only when caller passes `reveal_sensitive=true` and has `candidates:read` permission.
    pub phone: Option<String>,
    /// Present only when caller passes `reveal_sensitive=true` and has `candidates:read` permission.
    pub ssn_last4: Option<String>,
    pub resume_storage_key: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
