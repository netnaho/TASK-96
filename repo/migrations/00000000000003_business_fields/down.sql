DROP INDEX IF EXISTS idx_onboarding_items_required;
ALTER TABLE onboarding_items
    DROP COLUMN IF EXISTS item_due_date,
    DROP COLUMN IF EXISTS required;

ALTER TABLE offers
    DROP COLUMN IF EXISTS clause_version,
    DROP COLUMN IF EXISTS template_id;

DROP INDEX IF EXISTS idx_candidates_org;
ALTER TABLE candidates
    DROP COLUMN IF EXISTS organization_id;
