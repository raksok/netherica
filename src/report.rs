use crate::config::Config;
use crate::domain::DryRunRow;
use crate::error::{AppError, AppResult};
use crate::repository::Repository;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use rust_decimal::Decimal;
use rust_embed::RustEmbed;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tera::{Context, Tera};

const REPORT_TEMPLATE_PATH: &str = "templates/report.html.tera";
const THAI_FONT_PATH: &str = "Sarabun-Regular.ttf";
const REPORT_VERSION: &str = "v0.1";

#[derive(RustEmbed)]
#[folder = "asset/"]
struct ReportAssets;

#[derive(Debug, Clone)]
pub struct ReportRenderInput {
    pub source_filename: String,
    pub file_hash: String,
    pub generated_at_utc: DateTime<Utc>,
    pub period_start_utc: DateTime<Utc>,
    pub period_end_utc: DateTime<Utc>,
    pub rows: Vec<DryRunRow>,
    pub product_metadata: BTreeMap<String, ReportProductMetadata>,
    pub department_metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ReportProductMetadata {
    pub display_name: String,
    pub unit: String,
}

#[derive(Debug, Serialize)]
struct ReportTemplateRow {
    product_id: String,
    product_display_name: String,
    unit: String,
    opening_leftover: String,
    total_subunits_used: String,
    whole_units_output: String,
    closing_leftover: String,
    department_rows: Vec<ReportTemplateDepartmentRow>,
}

#[derive(Debug, Serialize)]
struct ReportTemplateDepartmentRow {
    consume_department_code: String,
    product_name: String,
    opening_leftover: String,
    borrowed: String,
    dispensed: String,
    issued: String,
    unit: String,
}

#[derive(Debug, Serialize)]
struct ReportDepartmentTotal {
    department: String,
    total: String,
}

/// Renders the ingestion report HTML from dry-run rows.
///
/// The output includes:
/// - Print CSS optimized for A4 output
/// - Embedded Sarabun Thai font (base64 data URL)
/// - BE (Buddhist Era) dates in the report header
/// - Per-product leftovers and department-level totals
pub fn render_report_html(input: &ReportRenderInput) -> AppResult<String> {
    let template_html = load_template(REPORT_TEMPLATE_PATH)?;
    let thai_font_base64 = load_font_base64(THAI_FONT_PATH)?;

    let mut ordered_rows = input.rows.clone();
    ordered_rows.sort_by(|a, b| a.product_id.cmp(&b.product_id));

    let rows = ordered_rows
        .iter()
        .map(|row| {
            let product_meta = input.product_metadata.get(&row.product_id);
            let product_display_name = product_meta
                .map(|meta| meta.display_name.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "-".to_string());
            let unit = product_meta
                .map(|meta| meta.unit.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "-".to_string());

            let mut department_breakdown = row.department_breakdown.clone();
            department_breakdown.sort_by(|a, b| a.department.cmp(&b.department));

            let mut department_rows = Vec::with_capacity(department_breakdown.len().max(1));
            for (idx, usage) in department_breakdown.iter().enumerate() {
                let consume_department_code = input
                    .department_metadata
                    .get(&usage.department)
                    .cloned()
                    .unwrap_or_else(|| usage.department.clone());

                department_rows.push(ReportTemplateDepartmentRow {
                    consume_department_code,
                    product_name: product_display_name.clone(),
                    opening_leftover: if idx == 0 {
                        decimal_to_string(row.opening_leftover)
                    } else {
                        String::new()
                    },
                    borrowed: String::new(),
                    dispensed: decimal_to_string(usage.quantity),
                    issued: String::new(),
                    unit: unit.clone(),
                });
            }

            if department_rows.is_empty() {
                department_rows.push(ReportTemplateDepartmentRow {
                    consume_department_code: "-".to_string(),
                    product_name: product_display_name.clone(),
                    opening_leftover: decimal_to_string(row.opening_leftover),
                    borrowed: String::new(),
                    dispensed: "0".to_string(),
                    issued: String::new(),
                    unit: unit.clone(),
                });
            }

            ReportTemplateRow {
                product_id: row.product_id.clone(),
                product_display_name,
                unit,
                opening_leftover: decimal_to_string(row.opening_leftover),
                total_subunits_used: decimal_to_string(row.total_subunits_used),
                whole_units_output: decimal_to_string(row.whole_units_output),
                closing_leftover: decimal_to_string(row.closing_leftover),
                department_rows,
            }
        })
        .collect::<Vec<_>>();

    let department_totals = aggregate_department_totals(&input.rows)
        .into_iter()
        .map(|(department, total)| ReportDepartmentTotal {
            department,
            total: decimal_to_string(total),
        })
        .collect::<Vec<_>>();

    let mut context = Context::new();
    context.insert("thai_font_base64", &thai_font_base64);
    context.insert("source_filename", &input.source_filename);
    context.insert("file_hash", &input.file_hash);
    let generated_at_be_local = format_be_datetime_local(input.generated_at_utc);
    context.insert("generated_at_be_local", &generated_at_be_local);
    context.insert(
        "period_start_be",
        &format_be_datetime(input.period_start_utc),
    );
    context.insert("period_end_be", &format_be_datetime(input.period_end_utc));
    context.insert("report_version", REPORT_VERSION);
    context.insert("rows", &rows);
    context.insert("department_totals", &department_totals);

    let mut tera = Tera::default();
    tera.add_raw_template(REPORT_TEMPLATE_PATH, &template_html)
        .map_err(|e| AppError::InternalError(format!("Failed to register report template: {e}")))?;

    tera.render(REPORT_TEMPLATE_PATH, &context)
        .map_err(|e| AppError::InternalError(format!("Failed to render report template: {e}")))
}

/// Renders and persists an HTML report to:
/// `reports/YYYYMMDD_HHMMSS_report.html`
pub fn render_and_save_report(input: &ReportRenderInput, reports_dir: &Path) -> AppResult<PathBuf> {
    let html = render_report_html(input)?;
    save_rendered_report(reports_dir, input.generated_at_utc, &html)
}

/// Persists a rendered report HTML string to the required path format:
/// `reports/YYYYMMDD_HHMMSS_report.html`
pub fn save_rendered_report(
    reports_dir: &Path,
    generated_at_utc: DateTime<Utc>,
    rendered_html: &str,
) -> AppResult<PathBuf> {
    fs::create_dir_all(reports_dir).map_err(AppError::IoError)?;

    let filename = format!("{}_report.html", generated_at_utc.format("%Y%m%d_%H%M%S"));
    let report_path = reports_dir.join(filename);
    fs::write(&report_path, rendered_html).map_err(AppError::IoError)?;

    Ok(report_path)
}

/// Regenerates the latest report by looking up the latest ingested `file_hash`
/// from file history and rebuilding report rows from persisted ledger data.
pub fn regenerate_last_report(
    repository: &Repository<'_>,
    config: &Config,
    reports_dir: &Path,
) -> AppResult<PathBuf> {
    let latest_hash = repository.get_latest_file_hash()?.ok_or_else(|| {
        AppError::DomainError("Cannot regenerate report: no file history found".to_string())
    })?;

    let file_history = repository
        .get_file_history_by_hash(&latest_hash)?
        .ok_or_else(|| {
            AppError::DomainError(format!(
                "Cannot regenerate report: missing file_history for hash {latest_hash}"
            ))
        })?;

    let entries = repository.get_ledger_entries_by_file_hash(&latest_hash)?;
    if entries.is_empty() {
        return Err(AppError::DomainError(format!(
            "Cannot regenerate report: no ledger entries found for hash {latest_hash}"
        )));
    }

    let period_start = entries
        .iter()
        .map(|entry| entry.transaction_date)
        .min()
        .ok_or_else(|| {
            AppError::InternalError(
                "Failed to determine report period start from ledger entries".to_string(),
            )
        })?;
    let period_end = entries
        .iter()
        .map(|entry| entry.transaction_date)
        .max()
        .ok_or_else(|| {
            AppError::InternalError(
                "Failed to determine report period end from ledger entries".to_string(),
            )
        })?;

    let rows = build_rows_from_entries(repository, config, &entries, period_start)?;
    let generated_at_utc = Utc::now();
    let input = ReportRenderInput {
        source_filename: file_history.filename,
        file_hash: latest_hash,
        generated_at_utc,
        period_start_utc: period_start,
        period_end_utc: period_end,
        rows,
        product_metadata: config
            .products
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    ReportProductMetadata {
                        display_name: p.display_name.clone(),
                        unit: p.unit.clone(),
                    },
                )
            })
            .collect(),
        department_metadata: config.departments.clone(),
    };

    render_and_save_report(&input, reports_dir)
}

fn load_template(path: &str) -> AppResult<String> {
    let template = ReportAssets::get(path).ok_or_else(|| {
        AppError::InternalError(format!("Missing embedded template asset: {path}"))
    })?;

    String::from_utf8(template.data.to_vec()).map_err(|e| {
        AppError::InternalError(format!("Template asset is not valid UTF-8 ({path}): {e}"))
    })
}

fn load_font_base64(path: &str) -> AppResult<String> {
    let font = ReportAssets::get(path)
        .ok_or_else(|| AppError::InternalError(format!("Missing embedded font asset: {path}")))?;

    Ok(BASE64_STANDARD.encode(font.data.as_ref()))
}

fn decimal_to_string(value: Decimal) -> String {
    value.normalize().to_string()
}

fn aggregate_department_totals(rows: &[DryRunRow]) -> BTreeMap<String, Decimal> {
    let mut totals = BTreeMap::<String, Decimal>::new();

    for row in rows {
        for usage in &row.department_breakdown {
            totals
                .entry(usage.department.clone())
                .and_modify(|sum| *sum += usage.quantity)
                .or_insert(usage.quantity);
        }
    }

    totals
}

fn format_be_datetime(dt: DateTime<Utc>) -> String {
    let be_year = dt.year() + 543;
    format!(
        "{:02}/{:02}/{:04} {:02}:{:02}",
        dt.day(),
        dt.month(),
        be_year,
        dt.hour(),
        dt.minute()
    )
}

fn format_be_datetime_local(dt: DateTime<Utc>) -> String {
    let local_dt = dt.with_timezone(&Local);
    let be_year = local_dt.year() + 543;
    format!(
        "{:02}/{:02}/{:04} {:02}:{:02}",
        local_dt.day(),
        local_dt.month(),
        be_year,
        local_dt.hour(),
        local_dt.minute()
    )
}

fn euclidean_mod(a: Decimal, n: Decimal) -> AppResult<Decimal> {
    if n <= Decimal::ZERO {
        return Err(AppError::DomainError(
            "Euclidean modulo divisor must be > 0".to_string(),
        ));
    }

    let r = a % n;
    if r.is_sign_negative() {
        Ok(r + n)
    } else {
        Ok(r)
    }
}

fn build_rows_from_entries(
    repository: &Repository<'_>,
    config: &Config,
    entries: &[crate::models::LedgerEntry],
    period_start: DateTime<Utc>,
) -> AppResult<Vec<DryRunRow>> {
    let factor_by_product = config
        .products
        .iter()
        .map(|p| (p.id.as_str(), p.factor))
        .collect::<BTreeMap<_, _>>();

    let mut usage_by_product = BTreeMap::<String, BTreeMap<String, Decimal>>::new();
    for entry in entries {
        usage_by_product
            .entry(entry.product_id.clone())
            .or_default()
            .entry(entry.department_id.clone())
            .and_modify(|sum| *sum += entry.dispensed_amount)
            .or_insert(entry.dispensed_amount);
    }

    let mut rows = Vec::new();
    for (product_id, dept_usage) in usage_by_product {
        let factor = factor_by_product
            .get(product_id.as_str())
            .copied()
            .ok_or_else(|| {
                AppError::DomainError(format!(
                    "Missing factor config for product '{}', cannot regenerate report",
                    product_id
                ))
            })?;

        let opening_total = repository.sum_before_date(&product_id, period_start)?;
        let opening_leftover = euclidean_mod(opening_total, factor)?;

        let mut total_subunits_used = Decimal::ZERO;
        let mut department_breakdown = Vec::new();
        for (department, qty) in dept_usage {
            total_subunits_used += qty;
            department_breakdown.push(crate::domain::DepartmentUsage {
                department,
                quantity: qty,
            });
        }

        let running_total = opening_leftover + total_subunits_used;
        let whole_units_output = (running_total / factor).floor();
        let closing_leftover = euclidean_mod(running_total, factor)?;

        rows.push(DryRunRow {
            product_id,
            department_breakdown,
            opening_leftover,
            total_subunits_used,
            whole_units_output,
            closing_leftover,
        });
    }

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColumnNames, Config, ProductConfig, Settings};
    use crate::db::Database;
    use crate::domain::DepartmentUsage;
    use crate::models::{FileHistory, LedgerEntry};
    use crate::repository::Repository;
    use chrono::TimeZone;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn euclidean_mod_normalizes_negative_dividend() {
        let result = euclidean_mod(Decimal::new(-1, 0), Decimal::new(5, 0))
            .expect("euclidean modulo should succeed for positive divisor");

        assert_eq!(result, Decimal::new(4, 0));
    }

    #[test]
    fn renders_report_with_be_dates_and_totals() {
        let input = ReportRenderInput {
            source_filename: "sample.xlsx".to_string(),
            file_hash: "abc123".to_string(),
            generated_at_utc: Utc.with_ymd_and_hms(2026, 4, 8, 12, 30, 0).unwrap(),
            period_start_utc: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
            period_end_utc: Utc.with_ymd_and_hms(2026, 4, 7, 23, 59, 0).unwrap(),
            rows: vec![DryRunRow {
                product_id: "P001".to_string(),
                department_breakdown: vec![
                    DepartmentUsage {
                        department: "ICU".to_string(),
                        quantity: Decimal::new(125, 1),
                    },
                    DepartmentUsage {
                        department: "ER".to_string(),
                        quantity: Decimal::new(25, 1),
                    },
                ],
                opening_leftover: Decimal::new(12, 1),
                total_subunits_used: Decimal::new(150, 1),
                whole_units_output: Decimal::new(15, 0),
                closing_leftover: Decimal::new(0, 1),
            }],
            product_metadata: BTreeMap::from([(
                "P001".to_string(),
                ReportProductMetadata {
                    display_name: "Product 001".to_string(),
                    unit: "PAIR".to_string(),
                },
            )]),
            department_metadata: BTreeMap::from([
                ("ER".to_string(), "[101] ER".to_string()),
                ("ICU".to_string(), "[202] ICU".to_string()),
            ]),
        };

        let html = render_report_html(&input).expect("report rendering should succeed");

        // BE year: 2026 + 543 = 2569
        assert!(html.contains("2569"));
        assert!(html.contains("sample.xlsx"));
        assert!(html.contains("P001"));
        assert!(html.contains("Product 001"));
        assert!(html.contains("[202] ICU"));
        assert!(html.contains("[101] ER"));
        assert!(html.contains("font-family: 'Sarabun'"));
        assert!(html.contains("Consume Department Code"));
        assert!(html.contains("Product name"));
        assert!(html.contains("ยอดยกมา"));
        assert!(html.contains("ขอยืม"));
        assert!(html.contains("เบิก"));
        assert!(html.contains("จ่าย"));
        assert!(html.contains("unit"));
        assert!(html.contains("Report version:</strong> v0.1"));
        assert!(html.contains("Generated at (BE, local):"));
        assert!(html.contains("A4 landscape"));

        let er_index = html
            .find("[101] ER")
            .expect("ER row should be rendered in department table");
        let icu_index = html
            .find("[202] ICU")
            .expect("ICU row should be rendered in department table");
        assert!(
            er_index < icu_index,
            "department rows should be deterministically sorted"
        );

        let header_count = html.matches("Processed filename:").count();
        assert_eq!(
            header_count, 1,
            "single product report should render one repeated page header"
        );
    }

    #[test]
    fn repeats_page_header_for_each_product_page() {
        let input = ReportRenderInput {
            source_filename: "sample.xlsx".to_string(),
            file_hash: "hash2".to_string(),
            generated_at_utc: Utc.with_ymd_and_hms(2026, 4, 8, 12, 30, 0).unwrap(),
            period_start_utc: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
            period_end_utc: Utc.with_ymd_and_hms(2026, 4, 7, 23, 59, 0).unwrap(),
            rows: vec![
                DryRunRow {
                    product_id: "P002".to_string(),
                    department_breakdown: vec![DepartmentUsage {
                        department: "ICU".to_string(),
                        quantity: Decimal::new(4, 0),
                    }],
                    opening_leftover: Decimal::new(0, 0),
                    total_subunits_used: Decimal::new(4, 0),
                    whole_units_output: Decimal::new(4, 0),
                    closing_leftover: Decimal::new(0, 0),
                },
                DryRunRow {
                    product_id: "P001".to_string(),
                    department_breakdown: vec![DepartmentUsage {
                        department: "ER".to_string(),
                        quantity: Decimal::new(2, 0),
                    }],
                    opening_leftover: Decimal::new(1, 0),
                    total_subunits_used: Decimal::new(2, 0),
                    whole_units_output: Decimal::new(3, 0),
                    closing_leftover: Decimal::new(0, 0),
                },
            ],
            product_metadata: BTreeMap::from([
                (
                    "P001".to_string(),
                    ReportProductMetadata {
                        display_name: "Product 001".to_string(),
                        unit: "PAIR".to_string(),
                    },
                ),
                (
                    "P002".to_string(),
                    ReportProductMetadata {
                        display_name: "Product 002".to_string(),
                        unit: "ROLL".to_string(),
                    },
                ),
            ]),
            department_metadata: BTreeMap::from([
                ("ER".to_string(), "[101] ER".to_string()),
                ("ICU".to_string(), "[202] ICU".to_string()),
            ]),
        };

        let html = render_report_html(&input).expect("report rendering should succeed");
        assert_eq!(html.matches("Processed filename:").count(), 2);
        assert_eq!(html.matches("Report version:</strong> v0.1").count(), 2);
        assert!(html.contains("Product 001"));
        assert!(html.contains("Product 002"));
        assert_eq!(html.matches("Consume Department Code").count(), 2);

        let p001_index = html
            .find("<p class=\"product-id\">P001</p>")
            .expect("P001 page should be present");
        let p002_index = html
            .find("<p class=\"product-id\">P002</p>")
            .expect("P002 page should be present");
        assert!(
            p001_index < p002_index,
            "product pages should be deterministically sorted by product_id"
        );

        assert!(
            !html.contains("<span>ขอยืม</span><strong>-</strong>"),
            "borrow summary must remain blank when borrow flow is not processed"
        );
        assert!(
            !html.contains("<span>จ่าย</span><strong>-</strong>"),
            "issued summary must remain blank when borrow flow is not processed"
        );
    }

    #[test]
    fn embedded_report_assets_are_available() {
        assert!(
            ReportAssets::get(REPORT_TEMPLATE_PATH).is_some(),
            "embedded template must exist"
        );
        assert!(
            ReportAssets::get(THAI_FONT_PATH).is_some(),
            "embedded font must exist"
        );
    }

    #[test]
    fn saves_report_to_required_file_naming_convention() {
        let temp = tempdir().expect("tempdir should be created");
        let reports_dir = temp.path().join("reports");

        let generated_at = Utc.with_ymd_and_hms(2026, 4, 8, 9, 10, 11).unwrap();
        let path = save_rendered_report(&reports_dir, generated_at, "<html>ok</html>")
            .expect("report should be saved");

        let expected = reports_dir.join("20260408_091011_report.html");
        assert_eq!(path, expected);
        assert!(expected.exists());
    }

    #[test]
    fn regenerates_last_report_from_latest_file_hash() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let reports_dir = temp.path().join("reports");

        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        let old_hash = "old_hash".to_string();
        let latest_hash = "latest_hash".to_string();

        // Prior file creates opening remainder for latest file (3 mod 2 = 1).
        repo.commit_ingestion_batch(
            &FileHistory {
                file_hash: old_hash.clone(),
                filename: "older.xlsx".to_string(),
                file_size: 123,
                transaction_date: chrono::NaiveDate::from_ymd_opt(2026, 4, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc(),
            },
            &[LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "Ward".to_string(),
                dispensed_amount: Decimal::new(3, 0),
                transaction_date: chrono::NaiveDate::from_ymd_opt(2026, 4, 1)
                    .unwrap()
                    .and_hms_opt(9, 0, 0)
                    .unwrap()
                    .and_utc(),
                file_hash: old_hash,
            }],
        )
        .expect("older ingestion should commit");

        repo.commit_ingestion_batch(
            &FileHistory {
                file_hash: latest_hash.clone(),
                filename: "latest.xlsx".to_string(),
                file_size: 456,
                transaction_date: chrono::NaiveDate::from_ymd_opt(2026, 4, 2)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc(),
            },
            &[
                LedgerEntry {
                    product_id: "P001".to_string(),
                    department_id: "ICU".to_string(),
                    dispensed_amount: Decimal::new(4, 0),
                    transaction_date: chrono::NaiveDate::from_ymd_opt(2026, 4, 2)
                        .unwrap()
                        .and_hms_opt(10, 0, 0)
                        .unwrap()
                        .and_utc(),
                    file_hash: latest_hash.clone(),
                },
                LedgerEntry {
                    product_id: "P001".to_string(),
                    department_id: "ER".to_string(),
                    dispensed_amount: Decimal::new(1, 0),
                    transaction_date: chrono::NaiveDate::from_ymd_opt(2026, 4, 2)
                        .unwrap()
                        .and_hms_opt(11, 0, 0)
                        .unwrap()
                        .and_utc(),
                    file_hash: latest_hash,
                },
            ],
        )
        .expect("latest ingestion should commit");

        let config = Config {
            database_path: db_path,
            settings: Settings {
                strict_chronological: true,
            },
            column_names: ColumnNames::default(),
            products: vec![ProductConfig {
                id: "P001".to_string(),
                display_name: "Product 001".to_string(),
                unit: "Box".to_string(),
                subunit: "Piece".to_string(),
                factor: Decimal::new(2, 0),
                track_subunits: true,
            }],
            departments: BTreeMap::from([
                ("ICU".to_string(), "Intensive Care".to_string()),
                ("ER".to_string(), "Emergency".to_string()),
                ("Ward".to_string(), "Ward".to_string()),
            ]),
        };

        let report_path = regenerate_last_report(&repo, &config, &reports_dir)
            .expect("report regeneration should succeed");

        assert!(report_path.exists());
        let file_name = report_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("report filename should be valid UTF-8");
        assert!(file_name.ends_with("_report.html"));

        let html = std::fs::read_to_string(&report_path).expect("report file should be readable");
        assert!(html.contains("latest.xlsx"));
        assert!(html.contains("latest_hash"));
        assert!(html.contains("P001"));
        assert!(html.contains("Product 001"));
        assert!(html.contains("Intensive Care"));
        assert!(html.contains("Emergency"));
    }
}
