use actix_web::{web, App, HttpServer};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use tracing::info;

use talentflow::api::{middleware::RequestId, routes};
use talentflow::infrastructure::{config::AppConfig, db, jobs, logging};
use talentflow::shared::app_state::AppState;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();
    logging::init();

    let config = AppConfig::from_env();
    info!(
        host = %config.host,
        port = config.port,
        "starting TalentFlow API server"
    );

    let pool = db::create_pool(&config.database_url);

    if config.run_migrations {
        info!("running pending database migrations");
        let mut conn = pool.get().expect("failed to get database connection");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("failed to run migrations");
        info!("migrations complete");
    }

    let state =
        AppState::new(config.clone(), pool).expect("failed to initialise application state");

    // Start background scheduler if enabled
    if config.scheduler.enabled {
        let scheduler_pool = db::create_pool(&config.database_url);
        let scheduler_config = config.clone();
        tokio::spawn(async move {
            jobs::start_scheduler(scheduler_pool, scheduler_config).await;
        });
    }

    let bind_addr = format!("{}:{}", config.host, config.port);
    let app_state = web::Data::new(state);

    HttpServer::new(move || {
        App::new()
            .wrap(RequestId)
            .app_data(app_state.clone())
            // Reject request bodies larger than 2 MB
            .app_data(
                web::JsonConfig::default()
                    .limit(2 * 1024 * 1024)
                    .error_handler(|err, _req| {
                        actix_web::Error::from(talentflow::shared::errors::AppError::Validation(
                            vec![talentflow::shared::errors::FieldError {
                                field: "body".into(),
                                message: err.to_string(),
                            }],
                        ))
                    }),
            )
            // Return JSON error envelope for malformed path parameters
            .app_data(web::PathConfig::default().error_handler(|err, _req| {
                actix_web::Error::from(talentflow::shared::errors::AppError::Validation(vec![
                    talentflow::shared::errors::FieldError {
                        field: "path".into(),
                        message: err.to_string(),
                    },
                ]))
            }))
            // Return JSON error envelope for malformed query parameters
            .app_data(web::QueryConfig::default().error_handler(|err, _req| {
                actix_web::Error::from(talentflow::shared::errors::AppError::Validation(vec![
                    talentflow::shared::errors::FieldError {
                        field: "query".into(),
                        message: err.to_string(),
                    },
                ]))
            }))
            .configure(routes::configure)
    })
    .bind(&bind_addr)?
    .run()
    .await
}
