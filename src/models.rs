use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistory {
    pub file_hash: String,
    pub filename: String,
    pub file_size: i64,
    pub transaction_date: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub product_id: String,
    pub department_id: String,
    pub dispensed_amount: Decimal,
    pub transaction_date: DateTime<Utc>,
    pub file_hash: String,
    pub borrowed_amount: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerRow {
    pub product_id: String,
    pub department_id: String,
    pub dispensed_amount: Decimal,
    pub transaction_date: DateTime<Utc>,
}
