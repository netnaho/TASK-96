use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConnectorType {
    Inbound,
    Outbound,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SyncStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub struct IntegrationConnector {
    pub id: Uuid,
    pub name: String,
    pub connector_type: ConnectorType,
    pub base_url: Option<String>,
    pub auth_config_encrypted: Option<Vec<u8>>,
    pub is_enabled: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct IntegrationSyncState {
    pub id: Uuid,
    pub connector_id: Uuid,
    pub entity_type: String,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_sync_cursor: Option<String>,
    pub status: SyncStatus,
    pub error_message: Option<String>,
    pub record_count: i32,
    pub updated_at: DateTime<Utc>,
}
