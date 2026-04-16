use chrono::NaiveDate;
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{
    DbOnboardingChecklist, DbOnboardingItem, NewDbOnboardingChecklist, NewDbOnboardingItem,
};
use crate::infrastructure::db::schema::{onboarding_checklists, onboarding_items};
use crate::shared::errors::AppError;

pub struct PgOnboardingRepository;

impl PgOnboardingRepository {
    // -------------------------------------------------------
    // Checklists
    // -------------------------------------------------------

    pub fn find_checklist(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<DbOnboardingChecklist>, AppError> {
        onboarding_checklists::table
            .filter(onboarding_checklists::id.eq(id))
            .select(DbOnboardingChecklist::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn create_checklist(
        conn: &mut PgConnection,
        new: NewDbOnboardingChecklist,
    ) -> Result<DbOnboardingChecklist, AppError> {
        diesel::insert_into(onboarding_checklists::table)
            .values(&new)
            .returning(DbOnboardingChecklist::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    pub fn list_checklists(
        conn: &mut PgConnection,
        candidate_id_filter: Option<Uuid>,
        assigned_to_filter: Option<Uuid>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<DbOnboardingChecklist>, i64), AppError> {
        use diesel::dsl::count_star;

        let mut base = onboarding_checklists::table.into_boxed();
        let mut count_q = onboarding_checklists::table.into_boxed();

        if let Some(cid) = candidate_id_filter {
            base = base.filter(onboarding_checklists::candidate_id.eq(cid));
            count_q = count_q.filter(onboarding_checklists::candidate_id.eq(cid));
        }

        if let Some(uid) = assigned_to_filter {
            base = base.filter(onboarding_checklists::assigned_to.eq(uid));
            count_q = count_q.filter(onboarding_checklists::assigned_to.eq(uid));
        }

        let total: i64 = count_q.select(count_star()).first(conn).map_err(db_err)?;

        let rows = base
            .select(DbOnboardingChecklist::as_select())
            .order(onboarding_checklists::created_at.desc())
            .offset(offset)
            .limit(limit)
            .load(conn)
            .map_err(db_err)?;

        Ok((rows, total))
    }

    // -------------------------------------------------------
    // Items
    // -------------------------------------------------------

    pub fn find_item(
        conn: &mut PgConnection,
        item_id: Uuid,
    ) -> Result<Option<DbOnboardingItem>, AppError> {
        onboarding_items::table
            .filter(onboarding_items::id.eq(item_id))
            .select(DbOnboardingItem::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn find_items_for_checklist(
        conn: &mut PgConnection,
        checklist_id: Uuid,
    ) -> Result<Vec<DbOnboardingItem>, AppError> {
        onboarding_items::table
            .filter(onboarding_items::checklist_id.eq(checklist_id))
            .select(DbOnboardingItem::as_select())
            .order(onboarding_items::item_order.asc())
            .load(conn)
            .map_err(db_err)
    }

    pub fn create_item(
        conn: &mut PgConnection,
        new: NewDbOnboardingItem,
    ) -> Result<DbOnboardingItem, AppError> {
        diesel::insert_into(onboarding_items::table)
            .values(&new)
            .returning(DbOnboardingItem::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Update status and optional completion metadata on an item.
    pub fn update_item_status(
        conn: &mut PgConnection,
        item_id: Uuid,
        status: &str,
        upload_storage_key: Option<&str>,
        health_attestation_encrypted: Option<&[u8]>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
        completed_by: Option<Uuid>,
    ) -> Result<DbOnboardingItem, AppError> {
        diesel::update(onboarding_items::table.filter(onboarding_items::id.eq(item_id)))
            .set((
                onboarding_items::status.eq(status),
                onboarding_items::upload_storage_key.eq(upload_storage_key),
                onboarding_items::health_attestation_encrypted.eq(health_attestation_encrypted),
                onboarding_items::completed_at.eq(completed_at),
                onboarding_items::completed_by.eq(completed_by),
            ))
            .returning(DbOnboardingItem::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Update the required flag and per-item due date (admin/club_admin operation).
    pub fn update_item_metadata(
        conn: &mut PgConnection,
        item_id: Uuid,
        required: bool,
        item_due_date: Option<NaiveDate>,
    ) -> Result<DbOnboardingItem, AppError> {
        diesel::update(onboarding_items::table.filter(onboarding_items::id.eq(item_id)))
            .set((
                onboarding_items::required.eq(required),
                onboarding_items::item_due_date.eq(item_due_date),
            ))
            .returning(DbOnboardingItem::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
