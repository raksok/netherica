use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::models::{FileHistory, LedgerEntry};
use chrono::{DateTime, Utc};
use rusqlite::{params, Transaction};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

pub struct Repository<'a> {
    db: &'a Database,
}

impl<'a> Repository<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    // --- File History ---

    pub fn insert_file_history(&self, tx: &Transaction<'a>, entry: &FileHistory) -> AppResult<()> {
        tx.execute(
            "INSERT INTO file_history (file_hash, filename, file_size, transaction_date) VALUES (?1, ?2, ?3, ?4)",
            params![
                entry.file_hash,
                entry.filename,
                entry.file_size,
                entry.transaction_date.to_rfc3339()
            ],
        )
        .map_err(AppError::DatabaseError)?;
        Ok(())
    }

    pub fn exists_by_hash(&self, hash: &str) -> AppResult<bool> {
        let exists = self
            .db
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM file_history WHERE file_hash = ?1)",
                params![hash],
                |row| row.get(0),
            )
            .map_err(AppError::DatabaseError)?;
        Ok(exists)
    }

    pub fn get_max_transaction_date(&self) -> AppResult<Option<DateTime<Utc>>> {
        let date_str: Option<String> = self
            .db
            .connection()
            .query_row(
                "SELECT MAX(transaction_date) FROM file_history",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::DatabaseError)?;

        match date_str {
            Some(s) => Ok(Some(parse_utc_db_datetime(&s)?)),
            None => Ok(None),
        }
    }

    pub fn get_latest_file_hash(&self) -> AppResult<Option<String>> {
        let res = self.db.connection().query_row(
            "SELECT file_hash FROM file_history ORDER BY transaction_date DESC, processed_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        );

        match res {
            Ok(hash) => Ok(Some(hash)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::DatabaseError(e)),
        }
    }

    pub fn get_file_history_by_hash(&self, hash: &str) -> AppResult<Option<FileHistory>> {
        let res = self.db.connection().query_row(
            "SELECT file_hash, filename, file_size, transaction_date FROM file_history WHERE file_hash = ?1",
            params![hash],
            |row| {
                let file_hash: String = row.get(0)?;
                let filename: String = row.get(1)?;
                let file_size: i64 = row.get(2)?;
                let transaction_date_str: String = row.get(3)?;

                let transaction_date =
                    parse_utc_db_datetime(&transaction_date_str).map_err(|_| rusqlite::Error::InvalidQuery)?;

                Ok(FileHistory {
                    file_hash,
                    filename,
                    file_size,
                    transaction_date,
                })
            },
        );

        match res {
            Ok(file_history) => Ok(Some(file_history)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::DatabaseError(e)),
        }
    }

    // --- Inventory Ledger ---

    pub fn batch_insert_ledger(
        &self,
        tx: &Transaction<'a>,
        entries: &[LedgerEntry],
    ) -> AppResult<()> {
        let mut stmt = tx.prepare(
            "INSERT INTO inventory_ledger (file_hash, product_id, department_id, dispensed_amount, transaction_date, borrowed_amount) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        ).map_err(AppError::DatabaseError)?;

        for entry in entries {
            stmt.execute(params![
                entry.file_hash,
                entry.product_id,
                entry.department_id,
                entry.dispensed_amount.to_string(),
                entry.transaction_date.to_rfc3339(),
                entry.borrowed_amount.to_string(),
            ])
            .map_err(AppError::DatabaseError)?;
        }
        Ok(())
    }

    pub fn sum_before_date_for_product_department(
        &self,
        product_id: &str,
        department_id: &str,
        date: DateTime<Utc>,
    ) -> AppResult<Decimal> {
        let sum_str: Option<String> = self
            .db
            .connection()
            .query_row(
                "SELECT CAST(SUM(dispensed_amount) AS TEXT)
             FROM inventory_ledger
             WHERE product_id = ?1 AND department_id = ?2 AND transaction_date < ?3",
                params![product_id, department_id, date.to_rfc3339()],
                |row| row.get(0),
            )
            .map_err(AppError::DatabaseError)?;

        match sum_str {
            Some(s) => {
                let val = s.parse::<Decimal>().map_err(|_| {
                    AppError::DomainError("Failed to parse quantity from database".to_string())
                })?;
                Ok(val)
            }
            None => Ok(Decimal::ZERO),
        }
    }

    pub fn sum_range_for_product_department(
        &self,
        product_id: &str,
        department_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> AppResult<Decimal> {
        let sum_str: Option<String> = self
            .db
            .connection()
            .query_row(
                "SELECT CAST(SUM(dispensed_amount) AS TEXT)
             FROM inventory_ledger
             WHERE product_id = ?1
               AND department_id = ?2
               AND transaction_date BETWEEN ?3 AND ?4",
                params![
                    product_id,
                    department_id,
                    start.to_rfc3339(),
                    end.to_rfc3339()
                ],
                |row| row.get(0),
            )
            .map_err(AppError::DatabaseError)?;

        match sum_str {
            Some(s) => {
                let val = s.parse::<Decimal>().map_err(|_| {
                    AppError::DomainError("Failed to parse quantity from database".to_string())
                })?;
                Ok(val)
            }
            None => Ok(Decimal::ZERO),
        }
    }

    pub fn get_ledger_entries_by_file_hash(&self, file_hash: &str) -> AppResult<Vec<LedgerEntry>> {
        let mut stmt = self
            .db
            .connection()
            .prepare(
                "SELECT product_id, department_id, CAST(dispensed_amount AS TEXT), transaction_date, file_hash, CAST(borrowed_amount AS TEXT)
                 FROM inventory_ledger
                 WHERE file_hash = ?1
                 ORDER BY transaction_date ASC, id ASC",
            )
            .map_err(AppError::DatabaseError)?;

        let rows = stmt
            .query_map(params![file_hash], |row| {
                let product_id: String = row.get(0)?;
                let department_id: String = row.get(1)?;
                let dispensed_amount_str: String = row.get(2)?;
                let transaction_date_str: String = row.get(3)?;
                let file_hash: String = row.get(4)?;
                let borrowed_amount_str: String = row.get(5)?;

                let dispensed_amount = dispensed_amount_str
                    .parse::<Decimal>()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let transaction_date = parse_utc_db_datetime(&transaction_date_str)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let borrowed_amount = borrowed_amount_str
                    .parse::<Decimal>()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;

                Ok(LedgerEntry {
                    product_id,
                    department_id,
                    dispensed_amount,
                    transaction_date,
                    file_hash,
                    borrowed_amount,
                })
            })
            .map_err(AppError::DatabaseError)?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(AppError::DatabaseError)?);
        }

        Ok(entries)
    }

    // --- Product Totals ---

    pub fn upsert_product_total(
        &self,
        tx: &Transaction<'a>,
        product_id: &str,
        department_id: &str,
        dispensed_amount: Decimal,
    ) -> AppResult<()> {
        tx.execute(
            "INSERT INTO product_totals (product_id, department_id, total_sum)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(product_id, department_id)
             DO UPDATE SET total_sum = total_sum + excluded.total_sum",
            params![product_id, department_id, dispensed_amount.to_string()],
        )
        .map_err(AppError::DatabaseError)?;
        Ok(())
    }

    /// Atomically commits one ingestion batch using a single SQLite transaction.
    ///
    /// Steps:
    /// 1) Insert one `file_history` row
    /// 2) Insert all `inventory_ledger` rows
    /// 3) Incrementally update `product_totals`
    ///
    /// On any failure, the transaction is rolled back and no partial writes are persisted.
    pub fn commit_ingestion_batch(
        &self,
        file_history: &FileHistory,
        entries: &[LedgerEntry],
    ) -> AppResult<()> {
        let tx = self
            .db
            .connection()
            .unchecked_transaction()
            .map_err(AppError::DatabaseError)?;

        let outcome = self.apply_ingestion_steps(&tx, file_history, entries);

        match outcome {
            Ok(()) => tx.commit().map_err(AppError::DatabaseError),
            Err(err) => {
                tx.rollback().map_err(AppError::DatabaseError)?;
                Err(err)
            }
        }
    }

    fn apply_ingestion_steps(
        &self,
        tx: &Transaction<'a>,
        file_history: &FileHistory,
        entries: &[LedgerEntry],
    ) -> AppResult<()> {
        self.insert_file_history(tx, file_history)?;
        self.batch_insert_ledger(tx, entries)?;

        let mut per_product_department_totals: BTreeMap<(&str, &str), Decimal> = BTreeMap::new();
        for entry in entries {
            if entry.dispensed_amount == Decimal::ZERO {
                continue;
            }

            per_product_department_totals
                .entry((entry.product_id.as_str(), entry.department_id.as_str()))
                .and_modify(|sum| *sum += entry.dispensed_amount)
                .or_insert(entry.dispensed_amount);
        }

        for ((product_id, department_id), total_sum) in per_product_department_totals {
            self.upsert_product_total(tx, product_id, department_id, total_sum)?;
        }

        Ok(())
    }

    pub fn get_total_for_product_department(
        &self,
        product_id: &str,
        department_id: &str,
    ) -> AppResult<Decimal> {
        let res = self.db.connection().query_row(
            "SELECT CAST(total_sum AS TEXT)
             FROM product_totals
             WHERE product_id = ?1 AND department_id = ?2",
            params![product_id, department_id],
            |row| row.get::<_, String>(0),
        );

        match res {
            Ok(s) => s.parse::<Decimal>().map_err(|_| {
                AppError::DomainError("Failed to parse quantity from database".to_string())
            }),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Decimal::ZERO),
            Err(e) => Err(AppError::DatabaseError(e)),
        }
    }

    pub fn get_totals_grouped_by_product_department(
        &self,
    ) -> AppResult<Vec<(String, String, Decimal)>> {
        let mut stmt = self
            .db
            .connection()
            .prepare(
                "SELECT product_id, department_id, CAST(total_sum AS TEXT)
                 FROM product_totals
                 ORDER BY product_id ASC, department_id ASC",
            )
            .map_err(AppError::DatabaseError)?;

        let rows = stmt
            .query_map([], |row| {
                let product_id: String = row.get(0)?;
                let department_id: String = row.get(1)?;
                let total_sum_str: String = row.get(2)?;
                let total_sum = total_sum_str
                    .parse::<Decimal>()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;

                Ok((product_id, department_id, total_sum))
            })
            .map_err(AppError::DatabaseError)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(AppError::DatabaseError)?);
        }

        Ok(out)
    }

    // --- Borrowed Carryover ---

    /// Retrieves the borrowed carryover amount for a specific product and department.
    /// Returns `Decimal::ZERO` if no record exists.
    pub fn get_borrowed_carryover(
        &self,
        product_id: &str,
        department_id: &str,
    ) -> AppResult<Decimal> {
        let res = self.db.connection().query_row(
            "SELECT CAST(amount AS TEXT)
             FROM borrowed_carryover
             WHERE product_id = ?1 AND department_id = ?2",
            params![product_id, department_id],
            |row| row.get::<_, String>(0),
        );

        match res {
            Ok(s) => s.parse::<Decimal>().map_err(|_| {
                AppError::DomainError("Failed to parse quantity from database".to_string())
            }),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Decimal::ZERO),
            Err(e) => Err(AppError::DatabaseError(e)),
        }
    }

    /// Inserts or updates the borrowed carryover amount for a specific product and department.
    /// Uses SQLite's INSERT ... ON CONFLICT DO UPDATE pattern.
    /// Updates the `updated_at` timestamp on conflict.
    pub fn upsert_borrowed_carryover(
        &self,
        tx: &Transaction<'a>,
        product_id: &str,
        department_id: &str,
        amount: Decimal,
    ) -> AppResult<()> {
        tx.execute(
            "INSERT INTO borrowed_carryover (product_id, department_id, amount)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(product_id, department_id)
             DO UPDATE SET amount = excluded.amount, updated_at = CURRENT_TIMESTAMP",
            params![product_id, department_id, amount.to_string()],
        )
        .map_err(AppError::DatabaseError)?;
        Ok(())
    }

    /// Atomically upserts borrowed carryover amounts for multiple
    /// (product_id, department_id) pairs.
    pub fn upsert_borrowed_carryover_batch(
        &self,
        updates: &[(String, String, Decimal)],
    ) -> AppResult<()> {
        let tx = self
            .db
            .connection()
            .unchecked_transaction()
            .map_err(AppError::DatabaseError)?;

        for (product_id, department_id, amount) in updates {
            self.upsert_borrowed_carryover(&tx, product_id, department_id, *amount)?;
        }

        tx.commit().map_err(AppError::DatabaseError)
    }

    pub fn get_nonzero_product_department_sums_before_date(
        &self,
        date: DateTime<Utc>,
    ) -> AppResult<Vec<(String, String, Decimal)>> {
        let mut stmt = self
            .db
            .connection()
            .prepare(
                "SELECT product_id, department_id, CAST(SUM(dispensed_amount) AS TEXT)
                 FROM inventory_ledger
                 WHERE transaction_date < ?1
                 GROUP BY product_id, department_id
                 HAVING SUM(dispensed_amount) != 0
                 ORDER BY product_id ASC, department_id ASC",
            )
            .map_err(AppError::DatabaseError)?;

        let rows = stmt
            .query_map(params![date.to_rfc3339()], |row| {
                let product_id: String = row.get(0)?;
                let department_id: String = row.get(1)?;
                let sum_str: String = row.get(2)?;
                let sum = sum_str
                    .parse::<Decimal>()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok((product_id, department_id, sum))
            })
            .map_err(AppError::DatabaseError)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(AppError::DatabaseError)?);
        }
        Ok(out)
    }
}

fn parse_utc_db_datetime(input: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(input)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| AppError::DomainError("Invalid datetime format in database".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use chrono::TimeZone;
    use tempfile::tempdir;

    fn sample_file_history(file_hash: &str) -> FileHistory {
        FileHistory {
            file_hash: file_hash.to_string(),
            filename: "sample.xlsx".to_string(),
            file_size: 128,
            transaction_date: Utc
                .with_ymd_and_hms(2026, 4, 1, 8, 0, 0)
                .single()
                .expect("timestamp should be valid"),
        }
    }

    fn sample_entries(file_hash: &str) -> Vec<LedgerEntry> {
        vec![
            LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ER".to_string(),
                dispensed_amount: Decimal::new(3, 0),
                transaction_date: Utc
                    .with_ymd_and_hms(2026, 4, 1, 8, 0, 0)
                    .single()
                    .expect("timestamp should be valid"),
                file_hash: file_hash.to_string(),
                borrowed_amount: Decimal::ZERO,
            },
            LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ICU".to_string(),
                dispensed_amount: Decimal::new(2, 0),
                transaction_date: Utc
                    .with_ymd_and_hms(2026, 4, 1, 9, 0, 0)
                    .single()
                    .expect("timestamp should be valid"),
                file_hash: file_hash.to_string(),
                borrowed_amount: Decimal::ZERO,
            },
        ]
    }

    fn sample_entries_with_borrowed(file_hash: &str) -> Vec<LedgerEntry> {
        vec![LedgerEntry {
            product_id: "P007".to_string(),
            department_id: "WARD".to_string(),
            dispensed_amount: Decimal::new(9, 0),
            transaction_date: Utc
                .with_ymd_and_hms(2026, 4, 2, 10, 30, 0)
                .single()
                .expect("timestamp should be valid"),
            file_hash: file_hash.to_string(),
            borrowed_amount: Decimal::new(125, 2),
        }]
    }

    #[test]
    fn commit_ingestion_batch_persists_all_tables_on_success() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        let file_hash = "hash-success";
        let history = sample_file_history(file_hash);
        let entries = sample_entries(file_hash);

        repo.commit_ingestion_batch(&history, &entries)
            .expect("commit should succeed");

        assert!(repo
            .exists_by_hash(file_hash)
            .expect("file_history query should succeed"));
        assert_eq!(
            repo.get_ledger_entries_by_file_hash(file_hash)
                .expect("ledger query should succeed")
                .len(),
            2
        );
        assert_eq!(
            repo.get_total_for_product_department("P001", "ER")
                .expect("product total query should succeed"),
            Decimal::new(3, 0)
        );
        assert_eq!(
            repo.get_total_for_product_department("P001", "ICU")
                .expect("product total query should succeed"),
            Decimal::new(2, 0)
        );
    }

    #[test]
    fn batch_insert_ledger_persists_borrowed_amount_column() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        let file_hash = "hash-borrowed-batch";
        let history = sample_file_history(file_hash);
        let entries = sample_entries_with_borrowed(file_hash);

        let tx = db
            .connection()
            .unchecked_transaction()
            .expect("transaction should start");
        repo.insert_file_history(&tx, &history)
            .expect("file history insert should succeed");
        repo.batch_insert_ledger(&tx, &entries)
            .expect("ledger batch insert should succeed");
        tx.commit().expect("commit should succeed");

        let borrowed_text: String = db
            .connection()
            .query_row(
                "SELECT borrowed_amount FROM inventory_ledger WHERE file_hash = ?1",
                params![file_hash],
                |row| row.get(0),
            )
            .expect("borrowed_amount should be persisted");

        assert_eq!(
            borrowed_text,
            Decimal::new(125, 2).to_string(),
            "borrowed_amount must be written as decimal text"
        );
    }

    #[test]
    fn get_ledger_entries_by_file_hash_reads_borrowed_amount() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        let file_hash = "hash-borrowed-read";
        let history = sample_file_history(file_hash);
        let entries = sample_entries_with_borrowed(file_hash);

        repo.commit_ingestion_batch(&history, &entries)
            .expect("commit should succeed");

        let read_entries = repo
            .get_ledger_entries_by_file_hash(file_hash)
            .expect("ledger query should succeed");

        assert_eq!(read_entries.len(), 1);
        assert_eq!(read_entries[0].borrowed_amount, Decimal::new(125, 2));
    }

    #[test]
    fn commit_ingestion_batch_rolls_back_all_writes_on_failure() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        db.connection()
            .execute_batch(
                "
                CREATE TRIGGER fail_product_total_insert
                BEFORE INSERT ON product_totals
                BEGIN
                    SELECT RAISE(ABORT, 'forced product_totals failure');
                END;
                ",
            )
            .expect("failure trigger should be created");

        let file_hash = "hash-rollback";
        let history = sample_file_history(file_hash);
        let entries = sample_entries(file_hash);

        let result = repo.commit_ingestion_batch(&history, &entries);
        assert!(matches!(result, Err(AppError::DatabaseError(_))));

        assert!(
            !repo
                .exists_by_hash(file_hash)
                .expect("file_history query should succeed"),
            "file_history row must rollback"
        );
        assert_eq!(
            repo.get_ledger_entries_by_file_hash(file_hash)
                .expect("ledger query should succeed")
                .len(),
            0,
            "ledger rows must rollback"
        );
        assert_eq!(
            repo.get_total_for_product_department("P001", "ER")
                .expect("product total query should succeed"),
            Decimal::ZERO,
            "product_totals upsert must rollback"
        );
    }

    // --- Borrowed Carryover Tests ---

    #[test]
    fn get_borrowed_carryover_returns_zero_when_no_record_exists() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        let amount = repo
            .get_borrowed_carryover("P001", "ER")
            .expect("query should succeed");

        assert_eq!(
            amount,
            Decimal::ZERO,
            "should return zero when no record exists"
        );
    }

    #[test]
    fn upsert_borrowed_carryover_inserts_new_record() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        let tx = db
            .connection()
            .unchecked_transaction()
            .expect("transaction should start");
        repo.upsert_borrowed_carryover(&tx, "P001", "ER", Decimal::new(15, 1))
            .expect("upsert should succeed");
        tx.commit().expect("commit should succeed");

        let amount = repo
            .get_borrowed_carryover("P001", "ER")
            .expect("query should succeed");

        assert_eq!(
            amount,
            Decimal::new(15, 1),
            "should retrieve inserted amount"
        );
    }

    #[test]
    fn upsert_borrowed_carryover_updates_existing_record() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        // Insert initial record
        let tx = db
            .connection()
            .unchecked_transaction()
            .expect("transaction should start");
        repo.upsert_borrowed_carryover(&tx, "P001", "ER", Decimal::new(10, 0))
            .expect("first upsert should succeed");
        tx.commit().expect("commit should succeed");

        // Update the same record
        let tx = db
            .connection()
            .unchecked_transaction()
            .expect("transaction should start");
        repo.upsert_borrowed_carryover(&tx, "P001", "ER", Decimal::new(25, 0))
            .expect("second upsert should succeed");
        tx.commit().expect("commit should succeed");

        let amount = repo
            .get_borrowed_carryover("P001", "ER")
            .expect("query should succeed");

        assert_eq!(
            amount,
            Decimal::new(25, 0),
            "should retrieve updated amount (not cumulative)"
        );
    }

    #[test]
    fn borrowed_carryover_supports_multiple_product_department_pairs() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        // Insert multiple records
        let tx = db
            .connection()
            .unchecked_transaction()
            .expect("transaction should start");
        repo.upsert_borrowed_carryover(&tx, "P001", "ER", Decimal::new(10, 0))
            .expect("upsert should succeed");
        repo.upsert_borrowed_carryover(&tx, "P001", "ICU", Decimal::new(20, 0))
            .expect("upsert should succeed");
        repo.upsert_borrowed_carryover(&tx, "P002", "ER", Decimal::new(30, 0))
            .expect("upsert should succeed");
        tx.commit().expect("commit should succeed");

        // Verify each record independently
        assert_eq!(
            repo.get_borrowed_carryover("P001", "ER")
                .expect("query should succeed"),
            Decimal::new(10, 0)
        );
        assert_eq!(
            repo.get_borrowed_carryover("P001", "ICU")
                .expect("query should succeed"),
            Decimal::new(20, 0)
        );
        assert_eq!(
            repo.get_borrowed_carryover("P002", "ER")
                .expect("query should succeed"),
            Decimal::new(30, 0)
        );
        assert_eq!(
            repo.get_borrowed_carryover("P002", "ICU")
                .expect("query should succeed"),
            Decimal::ZERO,
            "non-existent record should return zero"
        );
    }
}
