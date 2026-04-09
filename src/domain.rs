use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepartmentUsage {
    pub department: String,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRunRow {
    pub product_id: String,
    pub department_breakdown: Vec<DepartmentUsage>,
    pub opening_leftover: Decimal,
    pub total_subunits_used: Decimal,
    pub whole_units_output: Decimal,
    pub closing_leftover: Decimal,
}
