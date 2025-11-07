use diesel::prelude::*;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;

#[derive(Clone)]
pub struct Db {
    pub pool: Pool<SyncConnectionWrapper<SqliteConnection>>,
}

impl Db {
    /// Yes, this fuction can and WILL panic if it can't create the connection pool
    /// for some reason. We just want to bail because the appview
    /// does not work without a database.
    pub async fn new(db_path: Option<String>) -> Self {
        let database_url = if let Some(db_path) = db_path {
            db_path
        } else {
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set")
        };
        let config = AsyncDieselConnectionManager::<SyncConnectionWrapper<SqliteConnection>>::new(
            database_url,
        );
        let pool = Pool::builder(config)
            .build()
            .expect("Failed to create pool");
        Self { pool }
    }
}

pub fn run_migrations(
    db_path: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let database_url = if let Some(db_path) = db_path {
        db_path
    } else {
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set")
    };
    let mut connection = SqliteConnection::establish(&database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", database_url));
    // This will run the necessary migrations.
    //
    // See the documentation for `MigrationHarness` for
    // all available methods.
    println!("Attempting migrations...");
    let result = connection.run_pending_migrations(MIGRATIONS);
    println!("{:?}", result);
    if result.is_err() {
        println!("Failed to run migrations");
        return result.map(|_| ());
    }
    println!("Migrations Applied:");
    let applied_migrations = connection.applied_migrations()?;
    for migration in applied_migrations {
        println!("  * {}", migration);
    }
    Ok(())
}

pub struct Runtime;
