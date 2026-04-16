use diesel::prelude::*;
use diesel::sql_query;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use std::env;
use tracing::{info, warn};

use talentflow::infrastructure::{
    config::{self, AppConfig},
    crypto, db, logging,
};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Deterministic seed script.
///
/// Inserts baseline data required for the platform to function:
/// - System roles: guest, member, club_admin, platform_admin
/// - Base permissions for each resource/action pair
/// - Role-permission assignments
/// - One user per role for development/testing
/// - Controlled vocabulary examples
/// - Office site examples
///
/// This script is idempotent — it uses INSERT ... ON CONFLICT DO NOTHING
/// so it can be run repeatedly without error.
///
/// ## Seed user passwords
///
/// Passwords are read from environment variables.  If a variable is not set,
/// a random 24-character password is generated and printed to stdout once.
///
/// | Env var                    | Username        |
/// |----------------------------|-----------------|
/// | `SEED_ADMIN_PASSWORD`      | platform_admin  |
/// | `SEED_CLUB_ADMIN_PASSWORD` | club_admin      |
/// | `SEED_MEMBER_PASSWORD`     | member          |
///
/// For local dev convenience, set these in a `.env` file or pass them via
/// `docker-compose.dev.yml`.  **Never use known/static passwords in shared
/// environments.**

/// Resolve a seed password from an env var, or generate a random one.
/// Returns `(password, was_generated)`.
fn resolve_seed_password(env_var: &str) -> (String, bool) {
    let resolved = config::resolve_seed_password(env::var(env_var).ok().as_deref());
    (resolved.value, resolved.was_generated)
}

fn main() {
    dotenvy::dotenv().ok();
    logging::init();

    let config = AppConfig::from_env();
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get database connection");

    // Ensure migrations are applied before seeding
    info!("running pending migrations (if any)");
    conn.run_pending_migrations(MIGRATIONS)
        .expect("failed to run migrations");

    info!("seeding roles");
    sql_query(
        "INSERT INTO roles (id, name, description, is_system_role) VALUES
         ('a0000000-0000-0000-0000-000000000001', 'guest', 'Unauthenticated or minimal-access role', true),
         ('a0000000-0000-0000-0000-000000000002', 'member', 'Standard platform member', true),
         ('a0000000-0000-0000-0000-000000000003', 'club_admin', 'Club-level administrator', true),
         ('a0000000-0000-0000-0000-000000000004', 'platform_admin', 'Full platform administrator', true)
         ON CONFLICT (name) DO NOTHING",
    )
    .execute(&mut conn)
    .expect("failed to seed roles");

    info!("seeding permissions");
    let resources = [
        "users",
        "candidates",
        "offers",
        "approvals",
        "onboarding",
        "bookings",
        "sites",
        "reports",
        "reporting", // reporting mutations (subscriptions, dashboards, alerts)
        "integrations",
        "audit",
        "vocabularies",
        "search",
        "roles",
        "permissions",
    ];
    let actions = ["read", "create", "update", "delete"];
    for resource in &resources {
        for action in &actions {
            let q = format!(
                "INSERT INTO permissions (id, resource, action, description)
                 VALUES (gen_random_uuid(), '{resource}', '{action}', '{action} access to {resource}')
                 ON CONFLICT (resource, action) DO NOTHING"
            );
            sql_query(&q)
                .execute(&mut conn)
                .expect("failed to seed permission");
        }
    }

    info!("seeding role-permission mappings (platform_admin gets all)");
    sql_query(
        "INSERT INTO role_permissions (role_id, permission_id)
         SELECT 'a0000000-0000-0000-0000-000000000004', id FROM permissions
         ON CONFLICT DO NOTHING",
    )
    .execute(&mut conn)
    .expect("failed to seed role_permissions");

    // club_admin: read/create/update on candidates, offers, approvals, onboarding, bookings
    // reporting: create/read/update (can manage own subscriptions, acknowledge alerts, publish dashboards)
    info!("seeding club_admin permissions");
    sql_query(
        "INSERT INTO role_permissions (role_id, permission_id)
         SELECT 'a0000000-0000-0000-0000-000000000003', p.id
         FROM permissions p
         WHERE (p.resource IN ('candidates','offers','approvals','onboarding','bookings','sites','search','vocabularies')
                AND p.action IN ('read','create','update'))
            OR (p.resource = 'audit' AND p.action = 'read')
            OR (p.resource = 'reporting' AND p.action IN ('read','create','update'))
         ON CONFLICT DO NOTHING",
    )
    .execute(&mut conn)
    .expect("failed to seed club_admin permissions");

    // member: read own data only — enforced at object level in application layer
    // reporting: create/read (can manage own subscriptions; cannot publish dashboards or acknowledge alerts)
    info!("seeding member permissions");
    sql_query(
        "INSERT INTO role_permissions (role_id, permission_id)
         SELECT 'a0000000-0000-0000-0000-000000000002', p.id
         FROM permissions p
         WHERE (p.resource IN ('candidates','offers','onboarding','bookings','sites','search','vocabularies')
                AND p.action = 'read')
            OR (p.resource = 'reporting' AND p.action IN ('read','create'))
         ON CONFLICT DO NOTHING",
    )
    .execute(&mut conn)
    .expect("failed to seed member permissions");

    info!("seeding default users");

    let (admin_pw, admin_generated) = resolve_seed_password("SEED_ADMIN_PASSWORD");
    let (club_pw, club_generated) = resolve_seed_password("SEED_CLUB_ADMIN_PASSWORD");
    let (member_pw, member_generated) = resolve_seed_password("SEED_MEMBER_PASSWORD");

    // Print generated passwords exactly once so the operator can capture them.
    // This output goes to stdout (not logs) so it can be piped/captured.
    if admin_generated || club_generated || member_generated {
        warn!(
            "one or more seed passwords were auto-generated; \
             set SEED_ADMIN_PASSWORD / SEED_CLUB_ADMIN_PASSWORD / SEED_MEMBER_PASSWORD \
             to use explicit values"
        );
        println!("=== Generated seed passwords (store securely, shown once) ===");
        if admin_generated {
            println!("  platform_admin : {admin_pw}");
        }
        if club_generated {
            println!("  club_admin     : {club_pw}");
        }
        if member_generated {
            println!("  member         : {member_pw}");
        }
        println!("=============================================================");
    }

    let admin_hash = crypto::hash_password(&admin_pw).expect("failed to hash password");
    let club_hash = crypto::hash_password(&club_pw).expect("failed to hash password");
    let member_hash = crypto::hash_password(&member_pw).expect("failed to hash password");

    let user_insert = format!(
        "INSERT INTO users (id, username, email, password_hash, display_name) VALUES
         ('b0000000-0000-0000-0000-000000000001', 'platform_admin', 'admin@talentflow.local', '{admin_hash}', 'Platform Admin'),
         ('b0000000-0000-0000-0000-000000000002', 'club_admin', 'clubadmin@talentflow.local', '{club_hash}', 'Club Admin'),
         ('b0000000-0000-0000-0000-000000000003', 'member', 'member@talentflow.local', '{member_hash}', 'Member User')
         ON CONFLICT (username) DO NOTHING"
    );
    sql_query(&user_insert)
        .execute(&mut conn)
        .expect("failed to seed users");

    info!("seeding user-role assignments");
    sql_query(
        "INSERT INTO user_roles (user_id, role_id) VALUES
         ('b0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-000000000004'),
         ('b0000000-0000-0000-0000-000000000002', 'a0000000-0000-0000-0000-000000000003'),
         ('b0000000-0000-0000-0000-000000000003', 'a0000000-0000-0000-0000-000000000002')
         ON CONFLICT DO NOTHING",
    )
    .execute(&mut conn)
    .expect("failed to seed user_roles");

    info!("seeding controlled vocabularies");
    sql_query(
        "INSERT INTO controlled_vocabularies (id, category, value, label, sort_order) VALUES
         (gen_random_uuid(), 'department', 'engineering', 'Engineering', 1),
         (gen_random_uuid(), 'department', 'sales', 'Sales', 2),
         (gen_random_uuid(), 'department', 'hr', 'Human Resources', 3),
         (gen_random_uuid(), 'department', 'finance', 'Finance', 4),
         (gen_random_uuid(), 'candidate_source', 'referral', 'Employee Referral', 1),
         (gen_random_uuid(), 'candidate_source', 'job_board', 'Job Board', 2),
         (gen_random_uuid(), 'candidate_source', 'direct', 'Direct Application', 3),
         (gen_random_uuid(), 'candidate_tag', 'senior', 'Senior Level', 1),
         (gen_random_uuid(), 'candidate_tag', 'junior', 'Junior Level', 2),
         (gen_random_uuid(), 'candidate_tag', 'remote', 'Remote Eligible', 3)
         ON CONFLICT (category, value) DO NOTHING",
    )
    .execute(&mut conn)
    .expect("failed to seed vocabularies");

    info!("seeding office sites");
    sql_query(
        "INSERT INTO office_sites (id, code, name, address, latitude, longitude, timezone) VALUES
         (gen_random_uuid(), 'HQ', 'Headquarters', '100 Main St, Anytown, US 10001', 40.7128, -74.0060, 'America/New_York'),
         (gen_random_uuid(), 'WEST', 'West Coast Office', '200 Market St, San Francisco, CA 94105', 37.7749, -122.4194, 'America/Los_Angeles'),
         (gen_random_uuid(), 'REMOTE', 'Remote / Virtual', NULL, NULL, NULL, 'UTC')
         ON CONFLICT (code) DO NOTHING",
    )
    .execute(&mut conn)
    .expect("failed to seed sites");

    info!("seed complete");
}
