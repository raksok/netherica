use crate::error::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_database_path")]
    pub database_path: PathBuf,
    pub settings: Settings,
    #[serde(default = "default_column_names")]
    pub column_names: ColumnNames,
    pub products: Vec<ProductConfig>,
    pub departments: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColumnNames {
    pub date_visit: String,
    pub consume_department: String,
    pub code: String,
    pub qty: String,
}

impl Default for ColumnNames {
    fn default() -> Self {
        default_column_names()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProductConfig {
    pub id: String,
    pub display_name: String,
    pub unit: String,
    pub subunit: String,
    pub factor: Decimal,
    pub track_subunits: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub strict_chronological: bool,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default = "default_database_path")]
    database_path: PathBuf,
    #[serde(default = "default_settings")]
    settings: Settings,
    #[serde(default = "default_column_names")]
    column_names: ColumnNames,
    #[serde(default)]
    products: Vec<RawProductConfig>,
    #[serde(default)]
    departments: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawProductConfig {
    id: String,
    display_name: String,
    unit: String,
    subunit: String,
    factor: String,
    #[serde(default)]
    track_subunits: bool,
}

fn default_database_path() -> PathBuf {
    PathBuf::from("state.db")
}

fn default_settings() -> Settings {
    Settings {
        strict_chronological: true,
    }
}

fn default_column_names() -> ColumnNames {
    ColumnNames {
        date_visit: "Date Visit".to_string(),
        consume_department: "Consume Department".to_string(),
        code: "Code".to_string(),
        qty: "Qty".to_string(),
    }
}

impl ColumnNames {
    fn validate(&self) -> AppResult<()> {
        let fields = [
            ("column_names.date_visit", self.date_visit.trim()),
            (
                "column_names.consume_department",
                self.consume_department.trim(),
            ),
            ("column_names.code", self.code.trim()),
            ("column_names.qty", self.qty.trim()),
        ];

        for (field, value) in fields {
            if value.is_empty() {
                return Err(AppError::ConfigError(format!("{field} cannot be empty")));
            }
        }

        let normalized = [
            self.date_visit.trim().to_ascii_lowercase(),
            self.consume_department.trim().to_ascii_lowercase(),
            self.code.trim().to_ascii_lowercase(),
            self.qty.trim().to_ascii_lowercase(),
        ];

        let unique_count = normalized
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();

        if unique_count != normalized.len() {
            return Err(AppError::ConfigError(
                "column_names values must be unique (case-insensitive)".to_string(),
            ));
        }

        Ok(())
    }
}

impl Config {
    /// Loads configuration from `config.toml`.
    /// If the file does not exist, a default `config.toml` is generated.
    pub fn load() -> AppResult<Self> {
        let config_path = PathBuf::from("config.toml");

        if !config_path.exists() {
            fs::write(&config_path, Self::default_template())?;
            info!("Generated default config.toml");
        }

        let content = fs::read_to_string(&config_path)
            .map_err(|e| AppError::ConfigError(format!("Failed to read config.toml: {}", e)))?;

        let config = Self::from_toml_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn from_toml_str(content: &str) -> AppResult<Self> {
        let raw: RawConfig = toml::from_str(content)
            .map_err(|e| AppError::ConfigError(format!("Failed to parse config.toml: {}", e)))?;

        let mut products = Vec::with_capacity(raw.products.len());
        for product in raw.products {
            let factor = product.factor.parse::<Decimal>().map_err(|e| {
                AppError::ConfigError(format!(
                    "Product '{}' has invalid factor '{}': {}",
                    product.id, product.factor, e
                ))
            })?;

            products.push(ProductConfig {
                id: product.id,
                display_name: product.display_name,
                unit: product.unit,
                subunit: product.subunit,
                factor,
                track_subunits: product.track_subunits,
            });
        }

        Ok(Self {
            database_path: raw.database_path,
            settings: raw.settings,
            column_names: raw.column_names,
            products,
            departments: raw.departments,
        })
    }

    /// Validates the configuration rules.
    /// - Strict chronology must be enabled.
    /// - At least one product must be defined.
    /// - Product IDs must be unique.
    /// - Product factor must be > 0.
    /// - If track_subunits is false, factor must be exactly 1.
    /// - Departments must be non-empty and contain at least one entry.
    pub fn validate(&self) -> AppResult<()> {
        if !self.settings.strict_chronological {
            return Err(AppError::ConfigError(
                "settings.strict_chronological must be true".to_string(),
            ));
        }

        self.column_names.validate()?;

        if self.products.is_empty() {
            return Err(AppError::ConfigError(
                "At least one product must be defined".to_string(),
            ));
        }

        // 1. Product IDs are unique & Factor > 0
        let mut seen_ids = HashSet::new();
        for product in &self.products {
            if product.id.trim().is_empty() {
                return Err(AppError::ConfigError(
                    "Product id cannot be empty".to_string(),
                ));
            }

            if !seen_ids.insert(&product.id) {
                return Err(AppError::ConfigError(format!(
                    "Duplicate product ID found: {}",
                    product.id
                )));
            }

            if product.factor <= Decimal::ZERO {
                return Err(AppError::ConfigError(format!(
                    "Product '{}' must have factor > 0",
                    product.id
                )));
            }

            if !product.track_subunits && product.factor != Decimal::ONE {
                return Err(AppError::ConfigError(format!(
                    "Product '{}' has track_subunits = false, so factor must be 1",
                    product.id
                )));
            }

            if product.display_name.trim().is_empty() {
                return Err(AppError::ConfigError(format!(
                    "Product '{}' must have a non-empty display_name",
                    product.id
                )));
            }

            if product.unit.trim().is_empty() || product.subunit.trim().is_empty() {
                return Err(AppError::ConfigError(format!(
                    "Product '{}' must define both unit and subunit",
                    product.id
                )));
            }
        }

        // 2. Departments are non-empty
        if self.departments.is_empty() {
            return Err(AppError::ConfigError(
                "At least one department must be defined".to_string(),
            ));
        }

        for (dept_code, dept_name) in &self.departments {
            if dept_code.trim().is_empty() || dept_name.trim().is_empty() {
                return Err(AppError::ConfigError(
                    "Department code/name cannot be empty".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Warns if any configured products are missing from the provided list of Excel sheet names.
    pub fn warn_missing_sheets(&self, sheets_in_file: &[String]) {
        let sheet_set: HashSet<&str> = sheets_in_file.iter().map(String::as_str).collect();
        for product in &self.products {
            if !sheet_set.contains(product.id.as_str()) {
                warn!(
                    "Configured product '{}' sheet is missing in the provided file.",
                    product.id
                );
            }
        }
    }

    fn default_template() -> &'static str {
        r#"# Netherica config v0.1
[settings]
strict_chronological = true

[column_names]
date_visit = "Date Visit"
consume_department = "Consume Department"
code = "Code"
qty = "Qty"

[departments]
"[ER] Emergency" = "[205] Emergency"
"[HEMO] Hemodialysis Centre" = "[218] Hemodialysis Centre"
"[XRAY] Radiology Room" = "[XRAY] Radiology Room"
"[CU11] Checkup" = "[CHK] Checkup"
"[211M] OPD MED" = "[211] OPD MED"
"[S111] Surgery" = "[S111] Surgery"
"[Y111] TRUE C INSTITUTE" = "[211E] TRUE C INSTITUTE"
"[G111] Obstetrical & Gynecology" = "[OBST] Obstetrical & Gynecology"
"[C111] Pediatric" = "[211C] Pediatric"
"[4W] WARD 4" = "[4W] WARD 5"
"[C222] Well Baby" = "[211W] Well Baby"
"[K111] Skin Dept." = "[K111] Skin Dept."
"[101] Foreign Countries" = "[101] Foreign Countries"
"[PT] Physical Therapy" = "[PT] Physical Therapy"
"[O111] Orthopedic" = "[S222] Orthopedic"
"[OR] Operation Room" = "[208] Operation Room"
"[NS] Nursery" = "[NS] Nursery"
"[ICU] ICU" = "[ICU] ICU"
"[PP] Post Pratum" = "[PP] Post Pratum"
"[5W] WARD 5" = "[5W] WARD 6"
"[6W] WARD 6" = "[6W] WARD 7"
"[SGI] GI SCOPE" = "[SGI] GI SCOPE"
"[DNT] DENTAL" = "[213] DENTAL"
"[PHA] Pharmacy Room I" = "[105] Pharmacy Room I"
"[CATH] CATH LAB" = "[245] CATH LAB"

[[products]]
id = "2010100256"
display_name = "GLOVE DISPOSABLE SIZE XS/PAIR(คู่)@"
unit = "PAIR"
subunit = "PAIR"
factor = "1"
track_subunits = false

[[products]]
id = "2010100255"
display_name = "GLOVE DISPOSABLE SIZE S/PAIR(คู่)"
unit = "PAIR"
subunit = "PAIR"
factor = "1"
track_subunits = false

[[products]]
id = "2010100254"
display_name = "GLOVE DISPOSABLE SIZE M/PAIR(คู่)@"
unit = "PAIR"
subunit = "PAIR"
factor = "1"
track_subunits = false

[[products]]
id = "2010100253"
display_name = "GLOVE DISPOSABLE SIZE L/PAIR(คู่)@"
unit = "PAIR"
subunit = "PAIR"
factor = "1"
track_subunits = false

[[products]]
id = "2010101323"
display_name = "ET SHEATH ปลอกหุ้ม DIGITAL THERMOMETER TERUMO (ซองปรอท)@"
unit = "PC"
subunit = "PC"
factor = "1"
track_subunits = false

[[products]]
id = "1161106077"
display_name = "Chlorhexidine 2% in alcohol 30ml"
unit = "BOT"
subunit = "BOT"
factor = "1"
track_subunits = false

[[products]]
id = "ABN2100177"
display_name = "MICROPORE 1\" ตัดแบ่งเป็น CM(**)"
unit = "ROLL"
subunit = "CM."
factor = "900"
track_subunits = true

[[products]]
id = "ABN2100178"
display_name = "MICROPORE 1/2\" ตัดแบ่งเป็น CM(**)"
unit = "ROLL"
subunit = "CM."
factor = "900"
track_subunits = true

[[products]]
id = "ABN2100179"
display_name = "MICROPORE 1\" (มีที่ตัด) ตัดแบ่งเป็น CM(**)"
unit = "ROLL"
subunit = "CM."
factor = "900"
track_subunits = true

[[products]]
id = "ABN2100180"
display_name = "MICROPORE 1/2\" (มีที่ตัด) ตัดแบ่งเป็น CM(**)"
unit = "ROLL"
subunit = "CM."
factor = "900"
track_subunits = true

[[products]]
id = "ABN2100176"
display_name = "INNOTAPE (SILICONE TAPE) 2.5CM ตัดแบ่ง 5 CM(**)"
unit = "ROLL"
subunit = "5 CM."
factor = "30"
track_subunits = true

[[products]]
id = "ABN2100175"
display_name = "Disposible sheet laminate 80X200cm."
unit = "PC"
subunit = "PC"
factor = "1"
track_subunits = false

[[products]]
id = "ABN2100240"
display_name = "Antiseptic Tower/แผ่น ( No Alcohol )"
unit = "PC"
subunit = "PC"
factor = "1"
track_subunits = false

[[products]]
id = "ABN2100298"
display_name = "Antiseptic Towel /แผ่น"
unit = "PC"
subunit = "PC"
factor = "1"
track_subunits = false

[[products]]
id = "ABN2100302"
display_name = "Chlorhexidine 2% in water 15 ml (คิดเงิน)"
unit = "15 ml"
subunit = "15 ml"
factor = "1"
track_subunits = false
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config_toml() -> &'static str {
        r#"
[settings]
strict_chronological = true

[column_names]
date_visit = "Date Visit"
consume_department = "Consume Department"
code = "Code"
qty = "Qty"

[departments]
ER_CODE = "Emergency Room"
OPD_A = "Outpatient Ward"

[[products]]
id = "GAUZE-01"
display_name = "Gauze 500cm"
unit = "Roll"
subunit = "cm"
factor = "500"
track_subunits = true
"#
    }

    #[test]
    fn parses_required_v01_config_format() {
        let config = Config::from_toml_str(valid_config_toml()).expect("config should parse");
        config.validate().expect("config should validate");

        assert!(config.settings.strict_chronological);
        assert_eq!(config.column_names.date_visit, "Date Visit");
        assert_eq!(config.column_names.consume_department, "Consume Department");
        assert_eq!(config.column_names.code, "Code");
        assert_eq!(config.column_names.qty, "Qty");
        assert_eq!(config.products.len(), 1);
        assert_eq!(config.products[0].id, "GAUZE-01");
        assert_eq!(config.products[0].factor, Decimal::new(500, 0));
        assert_eq!(
            config.departments.get("ER_CODE"),
            Some(&"Emergency Room".to_string())
        );
    }

    #[test]
    fn rejects_when_products_are_missing() {
        let toml = r#"
[settings]
strict_chronological = true

[departments]
ER_CODE = "Emergency Room"
"#;

        let config = Config::from_toml_str(toml).expect("parsing should succeed");
        let err = config.validate().expect_err("validation should fail");
        assert!(err.to_string().contains("At least one product"));
    }

    #[test]
    fn rejects_track_subunits_false_with_factor_not_one() {
        let toml = r#"
[settings]
strict_chronological = true

[departments]
ER_CODE = "Emergency Room"

[[products]]
id = "ITEM-01"
display_name = "Item"
unit = "Box"
subunit = "Piece"
factor = "10"
track_subunits = false
"#;

        let config = Config::from_toml_str(toml).expect("parsing should succeed");
        let err = config.validate().expect_err("validation should fail");
        assert!(err.to_string().contains("factor must be 1"));
    }

    #[test]
    fn rejects_duplicate_product_ids() {
        let toml = r#"
[settings]
strict_chronological = true

[departments]
ER_CODE = "Emergency Room"

[[products]]
id = "P001"
display_name = "P1"
unit = "Box"
subunit = "Piece"
factor = "1"
track_subunits = true

[[products]]
id = "P001"
display_name = "P1 dup"
unit = "Box"
subunit = "Piece"
factor = "1"
track_subunits = true
"#;

        let config = Config::from_toml_str(toml).expect("parsing should succeed");
        let err = config.validate().expect_err("validation should fail");
        assert!(err.to_string().contains("Duplicate product ID"));
    }

    #[test]
    fn rejects_non_strict_chronological_setting() {
        let toml = r#"
[settings]
strict_chronological = false

[departments]
ER_CODE = "Emergency Room"

[[products]]
id = "P001"
display_name = "P1"
unit = "Box"
subunit = "Piece"
factor = "1"
track_subunits = true
"#;

        let config = Config::from_toml_str(toml).expect("parsing should succeed");
        let err = config.validate().expect_err("validation should fail");
        assert!(err
            .to_string()
            .contains("strict_chronological must be true"));
    }

    #[test]
    fn defaults_column_names_when_section_is_missing() {
        let toml = r#"
[settings]
strict_chronological = true

[departments]
ER_CODE = "Emergency Room"

[[products]]
id = "P001"
display_name = "P1"
unit = "Box"
subunit = "Piece"
factor = "1"
track_subunits = true
"#;

        let config = Config::from_toml_str(toml).expect("parsing should succeed");
        config.validate().expect("validation should succeed");
        assert_eq!(config.column_names.date_visit, "Date Visit");
        assert_eq!(config.column_names.consume_department, "Consume Department");
        assert_eq!(config.column_names.code, "Code");
        assert_eq!(config.column_names.qty, "Qty");
    }

    #[test]
    fn rejects_empty_column_name_values() {
        let toml = r#"
[settings]
strict_chronological = true

[column_names]
date_visit = " "
consume_department = "Consume Department"
code = "Code"
qty = "Qty"

[departments]
ER_CODE = "Emergency Room"

[[products]]
id = "P001"
display_name = "P1"
unit = "Box"
subunit = "Piece"
factor = "1"
track_subunits = true
"#;

        let config = Config::from_toml_str(toml).expect("parsing should succeed");
        let err = config.validate().expect_err("validation should fail");
        assert!(err
            .to_string()
            .contains("column_names.date_visit cannot be empty"));
    }

    #[test]
    fn rejects_duplicate_column_name_values_case_insensitive() {
        let toml = r#"
[settings]
strict_chronological = true

[column_names]
date_visit = "Code"
consume_department = "Consume Department"
code = "code"
qty = "Qty"

[departments]
ER_CODE = "Emergency Room"

[[products]]
id = "P001"
display_name = "P1"
unit = "Box"
subunit = "Piece"
factor = "1"
track_subunits = true
"#;

        let config = Config::from_toml_str(toml).expect("parsing should succeed");
        let err = config.validate().expect_err("validation should fail");
        assert!(err
            .to_string()
            .contains("column_names values must be unique"));
    }

    #[test]
    fn default_template_is_parseable_and_matches_appendix_defaults() {
        let config = Config::from_toml_str(Config::default_template())
            .expect("default template should parse");

        config.validate().expect("default template should validate");

        assert_eq!(config.departments.len(), 25);
        assert_eq!(config.products.len(), 15);

        assert_eq!(
            config.departments.get("[ER] Emergency"),
            Some(&"[205] Emergency".to_string())
        );

        let micropore = config
            .products
            .iter()
            .find(|p| p.id == "ABN2100177")
            .expect("ABN2100177 must exist");
        assert_eq!(micropore.unit, "ROLL");
        assert_eq!(micropore.subunit, "CM.");
        assert_eq!(micropore.factor, Decimal::new(900, 0));
        assert!(micropore.track_subunits);
    }
}
