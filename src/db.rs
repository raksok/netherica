use crate::error::{AppError, AppResult};
use rusqlite::Connection;
use std::path::Path;

pub struct Database {
    conn: Connection,
}

/// Incremental schema migrations keyed by `PRAGMA user_version`.
///
/// Each tuple is `(target_user_version, SQL_batch)` and is applied in order.
const MIGRATIONS: &[(u32, &str)] = &[
    (
        1,
        "
        CREATE TABLE IF NOT EXISTS file_history (
            file_hash TEXT PRIMARY KEY,
            filename TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            transaction_date DATETIME NOT NULL,
            processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS inventory_ledger (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_hash TEXT NOT NULL,
            product_id TEXT NOT NULL,
            department_id TEXT NOT NULL,
            dispensed_amount TEXT NOT NULL,
            transaction_date DATETIME NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
        );
        ",
    ),
    (
        2,
        "
        CREATE TABLE IF NOT EXISTS product_totals (
            product_id TEXT PRIMARY KEY,
            total_sum TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_ledger_product_date ON inventory_ledger(product_id, transaction_date);
        CREATE INDEX IF NOT EXISTS idx_ledger_file_hash ON inventory_ledger(file_hash);
        ",
    ),
];

const LATEST_SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

impl Database {
    /// Connects to the SQLite database at the given path and applies migrations.
    pub fn new(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path).map_err(AppError::DatabaseError)?;

        // Set WAL mode and synchronous settings for performance and reliability
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(AppError::DatabaseError)?;

        apply_migrations(&conn)?;

        Ok(Self { conn })
    }

    /// Provides access to the underlying connection (for use in transactions or repository functions).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

fn apply_migrations(conn: &Connection) -> AppResult<()> {
    let current_version = get_user_version(conn)?;

    if current_version > LATEST_SCHEMA_VERSION {
        return Err(AppError::InternalError(format!(
            "Database schema version {} is newer than this binary supports (max {}).",
            current_version, LATEST_SCHEMA_VERSION
        )));
    }

    for (target_version, sql) in MIGRATIONS {
        if *target_version <= current_version {
            continue;
        }

        let tx = conn.unchecked_transaction().map_err(|e| {
            AppError::InternalError(format!(
                "Failed to start migration transaction for version {}: {}",
                target_version, e
            ))
        })?;

        if let Err(e) = tx.execute_batch(sql) {
            return Err(AppError::InternalError(format!(
                "Migration to version {} failed while applying SQL: {}",
                target_version, e
            )));
        }

        if let Err(e) = tx.pragma_update(None, "user_version", target_version) {
            return Err(AppError::InternalError(format!(
                "Migration to version {} failed while updating user_version: {}",
                target_version, e
            )));
        }

        tx.commit().map_err(|e| {
            AppError::InternalError(format!(
                "Migration to version {} failed on commit: {}",
                target_version, e
            ))
        })?;
    }

    Ok(())
}

fn get_user_version(conn: &Connection) -> AppResult<u32> {
    conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .map_err(|e| AppError::InternalError(format!("Failed to read user_version: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn initializes_schema_and_sets_latest_user_version() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let db = Database::new(&db_path).expect("database should initialize");
        let conn = db.connection();

        let user_version = get_user_version(conn).expect("must read user_version");
        assert_eq!(user_version, LATEST_SCHEMA_VERSION);

        let journal_mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("must read journal_mode");
        assert_eq!(journal_mode.to_lowercase(), "wal");

        let synchronous: i64 = conn
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("must read synchronous");
        assert_eq!(synchronous, 1, "NORMAL synchronous pragma expected");
    }

    #[test]
    fn applies_only_pending_migrations_based_on_user_version() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let conn = Connection::open(&db_path).expect("connection should open");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS file_history (
                file_hash TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                transaction_date DATETIME NOT NULL,
                processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS inventory_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_hash TEXT NOT NULL,
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                dispensed_amount TEXT NOT NULL,
                transaction_date DATETIME NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
            );
            PRAGMA user_version = 1;
            ",
        )
        .expect("seed schema should be created");
        drop(conn);

        let db = Database::new(&db_path).expect("database should initialize from v1");
        let conn = db.connection();

        let user_version = get_user_version(conn).expect("must read user_version");
        assert_eq!(user_version, LATEST_SCHEMA_VERSION);

        let has_product_totals: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type='table' AND name='product_totals'",
                [],
                |row| row.get(0),
            )
            .expect("must query sqlite_master");
        assert_eq!(has_product_totals, 1);
    }

    #[test]
    fn fails_startup_clearly_when_migration_step_fails() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let conn = Connection::open(&db_path).expect("connection should open");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS file_history (
                file_hash TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                transaction_date DATETIME NOT NULL,
                processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS inventory_ledger (id INTEGER PRIMARY KEY AUTOINCREMENT);
            PRAGMA user_version = 1;
            ",
        )
        .expect("broken v1 schema should be seeded");
        drop(conn);

        let err = match Database::new(&db_path) {
            Ok(_) => panic!("migration should fail at startup"),
            Err(err) => err,
        };
        let msg = err.to_string();
        assert!(msg.contains("Migration to version 2 failed"));
    }
}
