use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::{
    models::{
        DbIntegrationConnector, DbIntegrationSyncState, NewDbIntegrationConnector,
        NewDbIntegrationSyncState,
    },
    schema::{integration_connectors, integration_sync_state},
};
use crate::shared::errors::AppError;

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}

pub struct PgIntegrationRepository;

impl PgIntegrationRepository {
    // ── Connectors ───────────────────────────────────────────────────────────

    pub fn insert_connector(
        conn: &mut PgConnection,
        record: NewDbIntegrationConnector,
    ) -> Result<DbIntegrationConnector, AppError> {
        diesel::insert_into(integration_connectors::table)
            .values(&record)
            .get_result(conn)
            .map_err(db_err)
    }

    pub fn find_connector(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<DbIntegrationConnector>, AppError> {
        integration_connectors::table
            .filter(integration_connectors::id.eq(id))
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn list_connectors(
        conn: &mut PgConnection,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbIntegrationConnector>, i64), AppError> {
        let offset = (page - 1).max(0) * per_page;
        let rows = integration_connectors::table
            .order(integration_connectors::created_at.desc())
            .limit(per_page)
            .offset(offset)
            .load(conn)
            .map_err(db_err)?;
        let total: i64 = integration_connectors::table
            .count()
            .get_result(conn)
            .map_err(db_err)?;
        Ok((rows, total))
    }

    pub fn update_connector(
        conn: &mut PgConnection,
        id: Uuid,
        name: Option<String>,
        base_url: Option<Option<String>>,
        is_enabled: Option<bool>,
        auth_config_encrypted: Option<Option<Vec<u8>>>,
    ) -> Result<DbIntegrationConnector, AppError> {
        // We can only set fields that were provided.  Use raw SQL for flexibility.
        diesel::sql_query(
            "UPDATE integration_connectors \
             SET name                   = COALESCE($2, name), \
                 base_url               = CASE WHEN $3::boolean THEN $4 ELSE base_url END, \
                 is_enabled             = COALESCE($5, is_enabled), \
                 auth_config_encrypted  = CASE WHEN $6::boolean THEN $7 ELSE auth_config_encrypted END, \
                 updated_at             = now() \
             WHERE id = $1 \
             RETURNING *",
        )
        .bind::<diesel::sql_types::Uuid, _>(id)
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(name)
        .bind::<diesel::sql_types::Bool, _>(base_url.is_some())
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(base_url.flatten())
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Bool>, _>(is_enabled)
        .bind::<diesel::sql_types::Bool, _>(auth_config_encrypted.is_some())
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Bytea>, _>(
            auth_config_encrypted.flatten(),
        )
        .get_result(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => AppError::NotFound("connector".into()),
            other => db_err(other),
        })
    }

    // ── Sync state ───────────────────────────────────────────────────────────

    pub fn get_sync_state(
        conn: &mut PgConnection,
        connector_id: Uuid,
    ) -> Result<Vec<DbIntegrationSyncState>, AppError> {
        integration_sync_state::table
            .filter(integration_sync_state::connector_id.eq(connector_id))
            .order(integration_sync_state::entity_type.asc())
            .load(conn)
            .map_err(db_err)
    }

    /// Return the sync state row for a single (connector_id, entity_type) pair.
    ///
    /// Used by the idempotency guard to check whether a sync is already running
    /// before accepting a new trigger request.
    pub fn find_sync_state_for_entity(
        conn: &mut PgConnection,
        connector_id: Uuid,
        entity_type: &str,
    ) -> Result<Option<DbIntegrationSyncState>, AppError> {
        integration_sync_state::table
            .filter(
                integration_sync_state::connector_id
                    .eq(connector_id)
                    .and(integration_sync_state::entity_type.eq(entity_type)),
            )
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    /// Upsert a sync state record for (connector_id, entity_type).
    /// If no row exists, insert; otherwise update.
    pub fn upsert_sync_state(
        conn: &mut PgConnection,
        connector_id: Uuid,
        entity_type: &str,
        status: &str,
        last_sync_at: Option<DateTime<Utc>>,
        last_sync_cursor: Option<String>,
        record_count: i32,
        error_message: Option<String>,
    ) -> Result<DbIntegrationSyncState, AppError> {
        diesel::sql_query(
            "INSERT INTO integration_sync_state \
               (id, connector_id, entity_type, status, last_sync_at, last_sync_cursor, \
                record_count, error_message, updated_at) \
             VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, now()) \
             ON CONFLICT (connector_id, entity_type) DO UPDATE SET \
               status            = EXCLUDED.status, \
               last_sync_at      = EXCLUDED.last_sync_at, \
               last_sync_cursor  = EXCLUDED.last_sync_cursor, \
               record_count      = EXCLUDED.record_count, \
               error_message     = EXCLUDED.error_message, \
               updated_at        = now() \
             RETURNING *",
        )
        .bind::<diesel::sql_types::Uuid, _>(connector_id)
        .bind::<diesel::sql_types::Text, _>(entity_type)
        .bind::<diesel::sql_types::Text, _>(status)
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>, _>(last_sync_at)
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(last_sync_cursor)
        .bind::<diesel::sql_types::Integer, _>(record_count)
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(error_message)
        .get_result(conn)
        .map_err(db_err)
    }

    /// Return the watermark (last_sync_at and last_sync_cursor) for incremental sync.
    pub fn get_watermark(
        conn: &mut PgConnection,
        connector_id: Uuid,
        entity_type: &str,
    ) -> Result<Option<(Option<DateTime<Utc>>, Option<String>)>, AppError> {
        integration_sync_state::table
            .filter(
                integration_sync_state::connector_id
                    .eq(connector_id)
                    .and(integration_sync_state::entity_type.eq(entity_type)),
            )
            .select((
                integration_sync_state::last_sync_at,
                integration_sync_state::last_sync_cursor,
            ))
            .first::<(Option<DateTime<Utc>>, Option<String>)>(conn)
            .optional()
            .map_err(db_err)
    }
}
