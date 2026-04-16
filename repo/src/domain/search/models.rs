use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A saved search query for the historical queries dictionary.
#[derive(Debug, Clone)]
pub struct HistoricalQuery {
    pub id: Uuid,
    pub user_id: Uuid,
    pub query_text: String,
    pub filters: serde_json::Value,
    pub result_count: Option<i32>,
    pub executed_at: DateTime<Utc>,
}

/// Search request parameters shared across searchable resources.
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub q: Option<String>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

/// Controlled vocabulary entry used for tag validation and autocomplete.
#[derive(Debug, Clone, Serialize)]
pub struct VocabularyEntry {
    pub id: Uuid,
    pub category: String,
    pub value: String,
    pub label: String,
    pub sort_order: i32,
}
