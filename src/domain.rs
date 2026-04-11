use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRunRow {
    pub product_id: String,
    pub product_display_name: String,
    pub department_id: String,
    pub department_display_name: String,
    pub opening_leftover: Decimal,
    pub borrowed: Decimal,
    pub total_subunits_used: Decimal,
    pub issued: Decimal,
    pub whole_units_output: Decimal,
    pub closing_leftover: Decimal,
}
