use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, Transaction};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
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
            product_id TEXT NOT NULL,
            department_id TEXT NOT NULL,
            total_sum TEXT NOT NULL,
            PRIMARY KEY (product_id, department_id)
        );

        CREATE INDEX IF NOT EXISTS idx_ledger_product_department_date ON inventory_ledger(product_id, department_id, transaction_date);
        CREATE INDEX IF NOT EXISTS idx_ledger_file_hash ON inventory_ledger(file_hash);
        ",
    ),
    (
        3,
        "
        ALTER TABLE inventory_ledger ADD COLUMN borrowed_amount TEXT NOT NULL DEFAULT '0';

        CREATE TABLE IF NOT EXISTS borrowed_carryover (
            product_id TEXT NOT NULL,
            department_id TEXT NOT NULL,
            amount TEXT NOT NULL DEFAULT '0',
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (product_id, department_id)
        );
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
            let _ = tx.rollback();
            return Err(AppError::InternalError(format!(
                "Migration to version {} failed while applying SQL: {}",
                target_version, e
            )));
        }

        if *target_version == 2 {
            if let Err(e) = migrate_product_totals_v2(&tx) {
                let _ = tx.rollback();
                return Err(AppError::InternalError(format!(
                    "Migration to version {} failed during product_totals migration: {}",
                    target_version, e
                )));
            }
        }

        if let Err(e) = tx.pragma_update(None, "user_version", target_version) {
            let _ = tx.rollback();
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

fn backfill_product_totals_from_ledger(tx: &Transaction<'_>) -> AppResult<()> {
    backfill_product_totals_from_ledger_into(tx, "product_totals")
}

fn migrate_product_totals_v2(tx: &Transaction<'_>) -> AppResult<()> {
    if has_legacy_product_totals_schema(tx)? {
        tx.execute_batch(
            "
            CREATE TABLE product_totals_new (
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                total_sum TEXT NOT NULL,
                PRIMARY KEY (product_id, department_id)
            );
            ",
        )
        .map_err(AppError::DatabaseError)?;

        backfill_product_totals_from_ledger_into(tx, "product_totals_new")?;

        tx.execute_batch(
            "
            DROP TABLE product_totals;
            ALTER TABLE product_totals_new RENAME TO product_totals;
            ",
        )
        .map_err(AppError::DatabaseError)?;

        tx.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_ledger_product_department_date ON inventory_ledger(product_id, department_id, transaction_date);
            CREATE INDEX IF NOT EXISTS idx_ledger_file_hash ON inventory_ledger(file_hash);
            ",
        )
        .map_err(AppError::DatabaseError)?;

        return Ok(());
    }

    backfill_product_totals_from_ledger(tx)
}

fn has_legacy_product_totals_schema(tx: &Transaction<'_>) -> AppResult<bool> {
    let table_exists: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type='table' AND name='product_totals'",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::DatabaseError)?;

    if table_exists == 0 {
        return Ok(false);
    }

    let mut stmt = tx
        .prepare("PRAGMA table_info(product_totals)")
        .map_err(AppError::DatabaseError)?;

    let rows = stmt
        .query_map([], |row| {
            let column_name: String = row.get(1)?;
            let pk_order: i64 = row.get(5)?;
            Ok((column_name, pk_order))
        })
        .map_err(AppError::DatabaseError)?;

    let mut has_department_id = false;
    let mut pk_columns = BTreeMap::<i64, String>::new();

    for row in rows {
        let (column_name, pk_order) = row.map_err(AppError::DatabaseError)?;
        if column_name == "department_id" {
            has_department_id = true;
        }
        if pk_order > 0 {
            pk_columns.insert(pk_order, column_name);
        }
    }

    let is_expected_composite_pk = pk_columns.len() == 2
        && pk_columns.get(&1).map(String::as_str) == Some("product_id")
        && pk_columns.get(&2).map(String::as_str) == Some("department_id");

    Ok(!has_department_id || !is_expected_composite_pk)
}

fn backfill_product_totals_from_ledger_into(
    tx: &Transaction<'_>,
    target_table: &str,
) -> AppResult<()> {
    let insert_sql = format!(
        "INSERT INTO {} (product_id, department_id, total_sum) VALUES (?1, ?2, ?3)",
        target_table
    );

    let mut stmt = tx
        .prepare(
            "SELECT product_id, department_id, GROUP_CONCAT(dispensed_amount, '|')
             FROM inventory_ledger
             GROUP BY product_id, department_id",
        )
        .map_err(AppError::DatabaseError)?;

    let rows = stmt
        .query_map([], |row| {
            let product_id: String = row.get(0)?;
            let department_id: String = row.get(1)?;
            let grouped_amounts: String = row.get(2)?;
            Ok((product_id, department_id, grouped_amounts))
        })
        .map_err(AppError::DatabaseError)?;

    let mut grouped = BTreeMap::<(String, String), Decimal>::new();
    for row in rows {
        let (product_id, department_id, grouped_amounts) = row.map_err(AppError::DatabaseError)?;

        let mut amount_total = Decimal::ZERO;
        for amount_str in grouped_amounts.split('|') {
            let amount = amount_str.parse::<Decimal>().map_err(|_| {
                AppError::InternalError(format!(
                    "Invalid decimal in inventory_ledger during backfill: {}",
                    amount_str
                ))
            })?;
            amount_total += amount;
        }

        grouped
            .entry((product_id, department_id))
            .and_modify(|sum| *sum += amount_total)
            .or_insert(amount_total);
    }

    let mut insert_stmt = tx.prepare(&insert_sql).map_err(AppError::DatabaseError)?;

    for ((product_id, department_id), total_sum) in grouped {
        insert_stmt
            .execute(params![product_id, department_id, total_sum.to_string()])
            .map_err(AppError::DatabaseError)?;
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

        let has_composite_index: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type='index' AND name='idx_ledger_product_department_date'",
                [],
                |row| row.get(0),
            )
            .expect("must query sqlite_master");
        assert_eq!(has_composite_index, 1);
    }

    #[test]
    fn migrates_v1_to_v2_and_backfills_product_department_totals() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let conn = Connection::open(&db_path).expect("connection should open");
        conn.execute_batch(
            "
            CREATE TABLE file_history (
                file_hash TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                transaction_date DATETIME NOT NULL,
                processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE inventory_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_hash TEXT NOT NULL,
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                dispensed_amount TEXT NOT NULL,
                transaction_date DATETIME NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
            );
            INSERT INTO file_history (file_hash, filename, file_size, transaction_date)
            VALUES ('f1', 'a.xlsx', 10, '2026-04-01T00:00:00+00:00');
            INSERT INTO inventory_ledger (file_hash, product_id, department_id, dispensed_amount, transaction_date)
            VALUES
                ('f1', 'P001', 'ER', '3', '2026-04-01T08:00:00+00:00'),
                ('f1', 'P001', 'ER', '2', '2026-04-01T09:00:00+00:00'),
                ('f1', 'P001', 'ICU', '5', '2026-04-01T10:00:00+00:00');
            PRAGMA user_version = 1;
            ",
        )
        .expect("seed v1 schema should be created");
        drop(conn);

        let db = Database::new(&db_path).expect("database should migrate from v1 to v2");
        let conn = db.connection();

        let totals: Vec<(String, String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT product_id, department_id, total_sum
                     FROM product_totals
                     ORDER BY product_id, department_id",
                )
                .expect("prepare should succeed");
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .expect("query should succeed");
            let mut data = Vec::new();
            for row in rows {
                data.push(row.expect("row should decode"));
            }
            data
        };

        assert_eq!(
            totals,
            vec![
                ("P001".to_string(), "ER".to_string(), "5".to_string()),
                ("P001".to_string(), "ICU".to_string(), "5".to_string()),
            ]
        );
    }

    #[test]
    fn migrates_legacy_product_totals_single_key_table_to_composite_key() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let conn = Connection::open(&db_path).expect("connection should open");
        conn.execute_batch(
            "
            CREATE TABLE file_history (
                file_hash TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                transaction_date DATETIME NOT NULL,
                processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE inventory_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_hash TEXT NOT NULL,
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                dispensed_amount TEXT NOT NULL,
                transaction_date DATETIME NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
            );
            CREATE TABLE product_totals (
                product_id TEXT PRIMARY KEY,
                total_sum TEXT NOT NULL
            );
            INSERT INTO product_totals (product_id, total_sum)
            VALUES ('P001', '999');
            INSERT INTO file_history (file_hash, filename, file_size, transaction_date)
            VALUES ('f1', 'a.xlsx', 10, '2026-04-01T00:00:00+00:00');
            INSERT INTO inventory_ledger (file_hash, product_id, department_id, dispensed_amount, transaction_date)
            VALUES
                ('f1', 'P001', 'ER', '3', '2026-04-01T08:00:00+00:00'),
                ('f1', 'P001', 'ER', '2', '2026-04-01T09:00:00+00:00'),
                ('f1', 'P001', 'ICU', '5', '2026-04-01T10:00:00+00:00');
            PRAGMA user_version = 1;
            ",
        )
        .expect("legacy schema should be seeded");
        drop(conn);

        let db = Database::new(&db_path).expect("database should migrate legacy product_totals");
        let conn = db.connection();

        let has_department_id: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM pragma_table_info('product_totals') WHERE name='department_id'",
                [],
                |row| row.get(0),
            )
            .expect("pragma_table_info query should succeed");
        assert_eq!(
            has_department_id, 1,
            "department_id must exist after migration"
        );

        let totals: Vec<(String, String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT product_id, department_id, total_sum
                     FROM product_totals
                     ORDER BY product_id, department_id",
                )
                .expect("prepare should succeed");
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .expect("query should succeed");
            let mut data = Vec::new();
            for row in rows {
                data.push(row.expect("row should decode"));
            }
            data
        };

        assert_eq!(
            totals,
            vec![
                ("P001".to_string(), "ER".to_string(), "5".to_string()),
                ("P001".to_string(), "ICU".to_string(), "5".to_string()),
            ],
            "backfill must come from inventory_ledger grouping, not legacy aggregate table"
        );
    }

    #[test]
    fn migration_failure_keeps_legacy_state_intact_without_partial_product_totals() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let conn = Connection::open(&db_path).expect("connection should open");
        conn.execute_batch(
            "
            CREATE TABLE file_history (
                file_hash TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                transaction_date DATETIME NOT NULL,
                processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE inventory_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_hash TEXT NOT NULL,
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                dispensed_amount TEXT NOT NULL,
                transaction_date DATETIME NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
            );
            CREATE TABLE product_totals (
                product_id TEXT PRIMARY KEY,
                total_sum TEXT NOT NULL
            );
            INSERT INTO product_totals (product_id, total_sum)
            VALUES ('P777', '77');
            INSERT INTO file_history (file_hash, filename, file_size, transaction_date)
            VALUES ('f1', 'broken.xlsx', 10, '2026-04-01T00:00:00+00:00');
            INSERT INTO inventory_ledger (file_hash, product_id, department_id, dispensed_amount, transaction_date)
            VALUES ('f1', 'P777', 'ER', 'NOT_A_DECIMAL', '2026-04-01T08:00:00+00:00');
            PRAGMA user_version = 1;
            ",
        )
        .expect("legacy schema with malformed ledger row should be seeded");
        drop(conn);

        let err = match Database::new(&db_path) {
            Ok(_) => panic!("migration should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("Migration to version 2 failed"),
            "error message should identify migration failure"
        );

        let conn = Connection::open(&db_path).expect("connection should reopen");

        let user_version =
            get_user_version(&conn).expect("must read user_version after failed migration");
        assert_eq!(
            user_version, 1,
            "failed migration must not advance user_version"
        );

        let has_department_id: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM pragma_table_info('product_totals') WHERE name='department_id'",
                [],
                |row| row.get(0),
            )
            .expect("pragma_table_info query should succeed");
        assert_eq!(
            has_department_id, 0,
            "product_totals must not be left in partially migrated shape"
        );

        let legacy_rows: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT product_id, total_sum FROM product_totals ORDER BY product_id")
                .expect("prepare should succeed");
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .expect("query should succeed");
            let mut data = Vec::new();
            for row in rows {
                data.push(row.expect("row should decode"));
            }
            data
        };

        assert_eq!(
            legacy_rows,
            vec![("P777".to_string(), "77".to_string())],
            "legacy product_totals content should remain recoverable"
        );

        let has_temp_replacement_table: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type='table' AND name='product_totals_new'",
                [],
                |row| row.get(0),
            )
            .expect("sqlite_master query should succeed");
        assert_eq!(
            has_temp_replacement_table, 0,
            "failed migration must not leave replacement table behind"
        );
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

    #[test]
    fn migrates_v2_to_v3_and_adds_borrowed_amount_column() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let conn = Connection::open(&db_path).expect("connection should open");
        conn.execute_batch(
            "
            CREATE TABLE file_history (
                file_hash TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                transaction_date DATETIME NOT NULL,
                processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE inventory_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_hash TEXT NOT NULL,
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                dispensed_amount TEXT NOT NULL,
                transaction_date DATETIME NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
            );
            CREATE TABLE product_totals (
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                total_sum TEXT NOT NULL,
                PRIMARY KEY (product_id, department_id)
            );
            CREATE INDEX idx_ledger_product_department_date ON inventory_ledger(product_id, department_id, transaction_date);
            CREATE INDEX idx_ledger_file_hash ON inventory_ledger(file_hash);
            INSERT INTO file_history (file_hash, filename, file_size, transaction_date)
            VALUES ('f1', 'a.xlsx', 10, '2026-04-01T00:00:00+00:00');
            INSERT INTO inventory_ledger (file_hash, product_id, department_id, dispensed_amount, transaction_date)
            VALUES ('f1', 'P001', 'ER', '3', '2026-04-01T08:00:00+00:00');
            PRAGMA user_version = 2;
            ",
        )
        .expect("seed v2 schema should be created");
        drop(conn);

        let db = Database::new(&db_path).expect("database should migrate from v2 to v3");
        let conn = db.connection();

        let user_version = get_user_version(conn).expect("must read user_version");
        assert_eq!(user_version, 3, "user_version should be 3 after migration");

        // Verify borrowed_amount column exists and has default value '0'
        let borrowed_amount: String = conn
            .query_row(
                "SELECT borrowed_amount FROM inventory_ledger WHERE product_id = 'P001'",
                [],
                |row| row.get(0),
            )
            .expect("should query borrowed_amount");

        assert_eq!(
            borrowed_amount, "0",
            "borrowed_amount should default to '0' for existing rows"
        );
    }

    #[test]
    fn migrates_v2_to_v3_and_creates_borrowed_carryover_table() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");

        let conn = Connection::open(&db_path).expect("connection should open");
        conn.execute_batch(
            "
            CREATE TABLE file_history (
                file_hash TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                transaction_date DATETIME NOT NULL,
                processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE inventory_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_hash TEXT NOT NULL,
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                dispensed_amount TEXT NOT NULL,
                transaction_date DATETIME NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(file_hash) REFERENCES file_history(file_hash)
            );
            CREATE TABLE product_totals (
                product_id TEXT NOT NULL,
                department_id TEXT NOT NULL,
                total_sum TEXT NOT NULL,
                PRIMARY KEY (product_id, department_id)
            );
            CREATE INDEX idx_ledger_product_department_date ON inventory_ledger(product_id, department_id, transaction_date);
            CREATE INDEX idx_ledger_file_hash ON inventory_ledger(file_hash);
            PRAGMA user_version = 2;
            ",
        )
        .expect("seed v2 schema should be created");
        drop(conn);

        let db = Database::new(&db_path).expect("database should migrate from v2 to v3");
        let conn = db.connection();

        let user_version = get_user_version(conn).expect("must read user_version");
        assert_eq!(user_version, 3, "user_version should be 3 after migration");

        // Verify borrowed_carryover table exists
        let table_exists: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type='table' AND name='borrowed_carryover'",
                [],
                |row| row.get(0),
            )
            .expect("must query sqlite_master");
        assert_eq!(table_exists, 1, "borrowed_carryover table should exist");

        // Verify expected columns exist
        let columns: Vec<(String, String, Option<String>, i64)> = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(borrowed_carryover)")
                .expect("prepare should succeed");
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(1)?,         // name
                        row.get::<_, String>(2)?,         // type
                        row.get::<_, Option<String>>(4)?, // dflt_value (can be NULL)
                        row.get::<_, i64>(5)?,            // pk
                    ))
                })
                .expect("query should succeed");
            let mut data = Vec::new();
            for row in rows {
                data.push(row.expect("row should decode"));
            }
            data
        };

        // Verify column names and types
        let column_map: std::collections::HashMap<String, (String, Option<String>, i64)> = columns
            .into_iter()
            .map(|(name, col_type, dflt, pk)| (name, (col_type, dflt, pk)))
            .collect();

        assert_eq!(
            column_map.get("product_id"),
            Some(&("TEXT".to_string(), None, 1)),
            "product_id should be TEXT and part of primary key"
        );
        assert_eq!(
            column_map.get("department_id"),
            Some(&("TEXT".to_string(), None, 2)),
            "department_id should be TEXT and part of primary key"
        );
        assert_eq!(
            column_map.get("amount"),
            Some(&("TEXT".to_string(), Some("'0'".to_string()), 0)),
            "amount should be TEXT with default '0'"
        );
        assert!(
            column_map.contains_key("updated_at"),
            "updated_at column should exist"
        );

        // Verify we can insert and query from the table
        conn.execute(
            "INSERT INTO borrowed_carryover (product_id, department_id, amount) VALUES (?1, ?2, ?3)",
            params!["P001", "ER", "10.5"],
        )
        .expect("insert should succeed");

        let amount: String = conn
            .query_row(
                "SELECT amount FROM borrowed_carryover WHERE product_id = 'P001' AND department_id = 'ER'",
                [],
                |row| row.get(0),
            )
            .expect("should query amount");

        assert_eq!(amount, "10.5", "should retrieve inserted amount");
    }
}
