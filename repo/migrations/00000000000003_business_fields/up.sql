-- ============================================================
-- Migration 3: Business module field additions
-- ============================================================

-- organization_id on candidates enables club_admin scope enforcement.
-- A club_admin with scope_type='organization'/scope_id=X can only access
-- candidates where organization_id = X.
ALTER TABLE candidates
    ADD COLUMN organization_id UUID;

CREATE INDEX idx_candidates_org ON candidates(organization_id);

-- Offer structured additions: template lineage and compensation blob.
-- Compensation is stored as AES-256-GCM encrypted JSON in the existing
-- compensation_encrypted column (already present from migration 1).
-- template_id / clause_version track which offer template and version was used.
ALTER TABLE offers
    ADD COLUMN template_id UUID,
    ADD COLUMN clause_version VARCHAR(32);

-- Onboarding items: required flag and per-item due date.
-- required=true items are counted in readiness_pct denominator.
-- item_due_date allows items to have individual deadlines within a checklist.
ALTER TABLE onboarding_items
    ADD COLUMN required BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN item_due_date DATE;

CREATE INDEX idx_onboarding_items_required ON onboarding_items(checklist_id, required);
