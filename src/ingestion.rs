use crate::config::{ColumnNames, Config};
use crate::domain::{DepartmentUsage, DryRunRow};
use crate::error::{AppError, AppResult};
use crate::models::{FileHistory, LedgerEntry, LedgerRow};
use crate::report::{self, ReportRenderInput};
use crate::repository::Repository;
use calamine::{open_workbook_auto, Data, Reader};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, Timelike, Utc};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

const DATE_VISIT_COL_IDX: usize = 4; // Column 5 (1-based)
const CONSUME_DEPARTMENT_COL_IDX: usize = 9; // Column 10 (1-based)
const CODE_COL_IDX: usize = 12; // Column 13 (1-based)
const QTY_COL_IDX: usize = 14; // Column 15 (1-based)

#[derive(Debug, Clone, Copy)]
struct ColumnIndexes {
    date_visit: usize,
    consume_department: usize,
    code: usize,
    qty: usize,
}

#[derive(Debug, Clone)]
pub struct IngestionOutcome {
    pub file_hash: String,
    pub report_path: PathBuf,
    pub archived_path: Option<PathBuf>,
    pub archive_move_pending: bool,
}

#[derive(Debug, Clone)]
pub struct ArchiveRetryResult {
    pub moved: Vec<PathBuf>,
    pub pending_count: usize,
}

#[derive(Debug, Clone)]
struct PendingArchiveMove {
    source_path: PathBuf,
    destination_path: PathBuf,
}

const ARCHIVE_PENDING_LIST_FILENAME: &str = "archive_retry_pending.txt";

#[derive(Debug, Clone)]
pub struct PendingIngestionCommit {
    pub source_file_path: PathBuf,
    pub file_hash: String,
    pub filename: String,
    pub file_size: i64,
    pub transaction_date: DateTime<Utc>,
    pub ledger_entries: Vec<LedgerEntry>,
    pub dry_run_rows: Vec<DryRunRow>,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub product_metadata: BTreeMap<String, report::ReportProductMetadata>,
    pub department_metadata: BTreeMap<String, String>,
    pub transaction_date_fallback_used: bool,
    pub transaction_date_warning: Option<String>,
}

pub fn prepare_ingestion_dry_run(
    file_path: &Path,
    config: &Config,
    repository: &Repository<'_>,
) -> AppResult<PendingIngestionCommit> {
    let file_hash = compute_file_hash(file_path)?;
    if repository.exists_by_hash(&file_hash)? {
        return Err(AppError::DomainError(
            "This file has already been processed.".to_string(),
        ));
    }

    let parsed = parse_excel_file(file_path, config)?;

    if let Some(existing_max) = repository.get_max_transaction_date()? {
        let new_date = parsed.earliest_transaction_utc;
        if new_date <= existing_max {
            return Err(AppError::ChronologicalViolation {
                new_date: new_date.date_naive(),
                existing_max: existing_max.date_naive(),
            });
        }
    }

    let filename = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::DomainError("Invalid UTF-8 filename".to_string()))?
        .to_string();
    let file_size = i64::try_from(fs::metadata(file_path).map_err(AppError::IoError)?.len())
        .map_err(|_| AppError::DomainError("File size exceeds supported range".to_string()))?;

    let ledger_entries = parsed
        .rows
        .iter()
        .map(|row| LedgerEntry {
            product_id: row.product_id.clone(),
            department_id: row.department_id.clone(),
            dispensed_amount: row.dispensed_amount,
            transaction_date: row.transaction_date,
            file_hash: file_hash.clone(),
        })
        .collect::<Vec<_>>();

    let period_start = parsed
        .rows
        .iter()
        .map(|r| r.transaction_date)
        .min()
        .ok_or_else(|| AppError::ExcelError("Parsed workbook has no valid rows".to_string()))?;
    let period_end = parsed
        .rows
        .iter()
        .map(|r| r.transaction_date)
        .max()
        .ok_or_else(|| AppError::ExcelError("Parsed workbook has no valid rows".to_string()))?;

    let dry_run_rows = build_dry_run_rows(repository, config, &parsed.rows)?;

    Ok(PendingIngestionCommit {
        source_file_path: file_path.to_path_buf(),
        file_hash,
        filename,
        file_size,
        transaction_date: parsed.earliest_transaction_utc,
        ledger_entries,
        dry_run_rows,
        period_start,
        period_end,
        product_metadata: build_report_product_metadata(config),
        department_metadata: config.departments.clone(),
        transaction_date_fallback_used: parsed.transaction_date_fallback_used,
        transaction_date_warning: parsed.transaction_date_warning,
    })
}

pub fn commit_prepared_ingestion(
    pending: &PendingIngestionCommit,
    repository: &Repository<'_>,
    reports_dir: &Path,
    archive_dir: &Path,
) -> AppResult<IngestionOutcome> {
    let file_history = FileHistory {
        file_hash: pending.file_hash.clone(),
        filename: pending.filename.clone(),
        file_size: pending.file_size,
        transaction_date: pending.transaction_date,
    };

    repository.commit_ingestion_batch(&file_history, &pending.ledger_entries)?;

    let report_input = ReportRenderInput {
        source_filename: pending.filename.clone(),
        file_hash: pending.file_hash.clone(),
        generated_at_utc: Utc::now(),
        period_start_utc: pending.period_start,
        period_end_utc: pending.period_end,
        rows: pending.dry_run_rows.clone(),
        product_metadata: pending.product_metadata.clone(),
        department_metadata: pending.department_metadata.clone(),
    };
    let report_path = report::render_and_save_report(&report_input, reports_dir)?;

    let archive_destination = build_archive_destination(&pending.source_file_path, archive_dir)?;
    let (archived_path, archive_move_pending) =
        match move_file_to_archive(&pending.source_file_path, &archive_destination) {
            Ok(()) => (Some(archive_destination), false),
            Err(err) => {
                warn!(
                    file = %pending.source_file_path.display(),
                    destination = %archive_destination.display(),
                    error = %err,
                    "Archive move failed after commit; move queued for retry"
                );
                queue_pending_archive_move(
                    archive_dir,
                    &pending.source_file_path,
                    &archive_destination,
                )?;
                (None, true)
            }
        };

    Ok(IngestionOutcome {
        file_hash: pending.file_hash.clone(),
        report_path,
        archived_path,
        archive_move_pending,
    })
}

pub fn ingest_excel_file(
    file_path: &Path,
    config: &Config,
    repository: &Repository<'_>,
    archive_dir: &Path,
    reports_dir: &Path,
) -> AppResult<IngestionOutcome> {
    let pending = prepare_ingestion_dry_run(file_path, config, repository)?;
    commit_prepared_ingestion(&pending, repository, reports_dir, archive_dir)
}

fn build_report_product_metadata(
    config: &Config,
) -> BTreeMap<String, report::ReportProductMetadata> {
    config
        .products
        .iter()
        .map(|product| {
            (
                product.id.clone(),
                report::ReportProductMetadata {
                    display_name: product.display_name.clone(),
                    unit: product.unit.clone(),
                },
            )
        })
        .collect()
}

fn compute_file_hash(path: &Path) -> AppResult<String> {
    let bytes = fs::read(path).map_err(AppError::IoError)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    Ok(format!("{:x}", digest))
}

struct ParsedWorkbook {
    rows: Vec<LedgerRow>,
    earliest_transaction_utc: DateTime<Utc>,
    transaction_date_fallback_used: bool,
    transaction_date_warning: Option<String>,
}

fn parse_excel_file(path: &Path, config: &Config) -> AppResult<ParsedWorkbook> {
    let file_mtime_utc = file_modified_time_utc(path)?;
    let mut workbook = open_workbook_auto(path)
        .map_err(|e| AppError::ExcelError(format!("Failed to open workbook: {e}")))?;

    let sheet_names = workbook.sheet_names().to_vec();
    config.warn_missing_sheets(&sheet_names);

    let configured_product_ids = config
        .products
        .iter()
        .map(|p| p.id.as_str())
        .collect::<HashSet<_>>();
    for sheet_name in &sheet_names {
        if !configured_product_ids.contains(sheet_name.as_str()) {
            warn!(
                sheet = %sheet_name,
                "Excel sheet is not configured and will be skipped"
            );
        }
    }

    let mut rows = Vec::<LedgerRow>::new();
    let mut earliest: Option<DateTime<Utc>> = None;
    let mut matching_sheets_found = 0usize;
    let mut sheet_with_required_columns_found = false;
    let mut fallback_rows = 0usize;

    for product in &config.products {
        if !sheet_names.iter().any(|name| name == &product.id) {
            continue;
        }

        matching_sheets_found += 1;

        let range = workbook.worksheet_range(&product.id).map_err(|e| {
            AppError::ExcelError(format!("Failed to read sheet '{}': {e}", product.id))
        })?;

        let Some(header_row) = range.rows().next() else {
            warn!(sheet = %product.id, "Sheet has no header row and will be skipped");
            continue;
        };

        let Some(column_indexes) =
            find_required_column_indexes(header_row, &product.id, &config.column_names)
        else {
            continue;
        };
        sheet_with_required_columns_found = true;

        if range.height() <= 1 {
            warn!(sheet = %product.id, "Sheet has no data rows and will be skipped");
            continue;
        }

        for (row_idx, row) in range.rows().skip(1).enumerate() {
            if row_is_empty(row) {
                continue;
            }

            let code = row
                .get(column_indexes.code)
                .and_then(cell_as_string)
                .unwrap_or_default();

            if code.trim().is_empty() {
                continue;
            }

            if code.trim() != product.id {
                warn!(
                    sheet = %product.id,
                    row_code = %code.trim(),
                    "Skipping row due to product ID mismatch"
                );
                continue;
            }

            let dispensed_amount = match row.get(column_indexes.qty) {
                Some(cell) => match parse_decimal(cell) {
                    Some(q) if q != Decimal::ZERO => q,
                    Some(_) => continue,
                    None => {
                        warn!(sheet = %product.id, "Skipping row due to non-numeric quantity");
                        continue;
                    }
                },
                None => {
                    warn!(sheet = %product.id, "Skipping row due to missing quantity value");
                    continue;
                }
            };

            let department_id = row
                .get(column_indexes.consume_department)
                .and_then(cell_as_string)
                .unwrap_or_default()
                .trim()
                .to_string();
            if department_id.is_empty() {
                warn!(sheet = %product.id, "Skipping row due to empty department value");
                continue;
            }

            let date_str = row
                .get(column_indexes.date_visit)
                .and_then(cell_as_string)
                .unwrap_or_default();
            let transaction_date = if date_str.trim().is_empty() {
                fallback_rows += 1;
                warn!(
                    sheet = %product.id,
                    row_number = row_idx + 2,
                    fallback_utc = %file_mtime_utc,
                    "Date missing; using file modification timestamp as UTC fallback"
                );
                file_mtime_utc
            } else {
                match be_to_gregorian(&date_str) {
                    Ok(value) => value,
                    Err(_) => {
                        fallback_rows += 1;
                        warn!(
                            sheet = %product.id,
                            row_number = row_idx + 2,
                            date = %date_str,
                            fallback_utc = %file_mtime_utc,
                            "Invalid Buddhist Era date; using file modification timestamp as UTC fallback"
                        );
                        file_mtime_utc
                    }
                }
            };

            earliest = Some(match earliest {
                Some(current) => current.min(transaction_date),
                None => transaction_date,
            });

            rows.push(LedgerRow {
                product_id: product.id.clone(),
                department_id,
                dispensed_amount,
                transaction_date,
            });
        }
    }

    if matching_sheets_found == 0 {
        return Err(AppError::ExcelError("No matching sheets.".to_string()));
    }

    if !sheet_with_required_columns_found {
        return Err(AppError::ExcelError(
            "Required column(s) not found.".to_string(),
        ));
    }

    if rows.is_empty() {
        return Err(AppError::ExcelError(
            "Workbook has no valid ledger rows".to_string(),
        ));
    }

    let fallback_warning = if fallback_rows > 0 {
        let message = format!(
            "Detected {fallback_rows} row(s) with missing/invalid dates. Using file modification time (UTC) {file_mtime_utc} as transaction date fallback. Please acknowledge before confirming commit."
        );
        warn!(
            file = %path.display(),
            fallback_rows,
            fallback_utc = %file_mtime_utc,
            "Applying transaction date fallback from file modification time"
        );
        Some(message)
    } else {
        None
    };

    Ok(ParsedWorkbook {
        rows,
        earliest_transaction_utc: earliest
            .ok_or_else(|| AppError::ExcelError("Workbook has no valid dates".to_string()))?,
        transaction_date_fallback_used: fallback_warning.is_some(),
        transaction_date_warning: fallback_warning,
    })
}

fn file_modified_time_utc(path: &Path) -> AppResult<DateTime<Utc>> {
    let modified = fs::metadata(path)
        .map_err(AppError::IoError)?
        .modified()
        .map_err(AppError::IoError)?;
    Ok(DateTime::<Utc>::from(modified))
}

pub fn be_to_gregorian(input: &str) -> AppResult<DateTime<Utc>> {
    let be_naive = NaiveDateTime::parse_from_str(input.trim(), "%d-%m-%Y %H:%M").map_err(|e| {
        AppError::ExcelError(format!(
            "Failed to parse BE datetime '{}': {e}",
            input.trim()
        ))
    })?;

    let gregorian_year = be_naive.year() - 543;
    let date = NaiveDate::from_ymd_opt(gregorian_year, be_naive.month(), be_naive.day())
        .ok_or_else(|| {
            AppError::ExcelError(format!(
                "Invalid Gregorian date after BE conversion: {input}"
            ))
        })?;
    let naive = date
        .and_hms_opt(be_naive.hour(), be_naive.minute(), be_naive.second())
        .ok_or_else(|| AppError::ExcelError(format!("Invalid time in datetime: {input}")))?;

    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

#[cfg(test)]
fn gregorian_to_be(dt: DateTime<Utc>) -> String {
    format!(
        "{:02}-{:02}-{:04} {:02}:{:02}",
        dt.day(),
        dt.month(),
        dt.year() + 543,
        dt.hour(),
        dt.minute()
    )
}

fn build_dry_run_rows(
    repository: &Repository<'_>,
    config: &Config,
    rows: &[LedgerRow],
) -> AppResult<Vec<DryRunRow>> {
    let factor_by_product = config
        .products
        .iter()
        .map(|p| (p.id.as_str(), p.factor))
        .collect::<BTreeMap<_, _>>();

    let mut usage_by_product = BTreeMap::<String, BTreeMap<String, Decimal>>::new();
    let period_start = rows
        .iter()
        .map(|r| r.transaction_date)
        .min()
        .ok_or_else(|| AppError::DomainError("No rows available for dry-run data".to_string()))?;

    for row in rows {
        usage_by_product
            .entry(row.product_id.clone())
            .or_default()
            .entry(row.department_id.clone())
            .and_modify(|sum| *sum += row.dispensed_amount)
            .or_insert(row.dispensed_amount);
    }

    let mut result = Vec::new();
    for (product_id, dept_usage) in usage_by_product {
        let factor = factor_by_product
            .get(product_id.as_str())
            .copied()
            .ok_or_else(|| {
                AppError::DomainError(format!("Missing product factor for '{}'", product_id))
            })?;

        let opening_total = repository.sum_before_date(&product_id, period_start)?;
        let opening_leftover = euclidean_mod(opening_total, factor)?;

        let mut total_subunits_used = Decimal::ZERO;
        let mut department_breakdown = Vec::new();
        for (department, qty) in dept_usage {
            total_subunits_used += qty;
            department_breakdown.push(DepartmentUsage {
                department,
                quantity: qty,
            });
        }

        let running_total = opening_leftover + total_subunits_used;
        let whole_units_output = (running_total / factor).floor();
        let closing_leftover = euclidean_mod(running_total, factor)?;

        result.push(DryRunRow {
            product_id,
            department_breakdown,
            opening_leftover,
            total_subunits_used,
            whole_units_output,
            closing_leftover,
        });
    }

    Ok(result)
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

pub fn retry_pending_archive_moves(archive_dir: &Path) -> AppResult<ArchiveRetryResult> {
    fs::create_dir_all(archive_dir).map_err(AppError::IoError)?;

    let mut pending = load_pending_archive_moves(archive_dir)?;
    if pending.is_empty() {
        return Ok(ArchiveRetryResult {
            moved: Vec::new(),
            pending_count: 0,
        });
    }

    let mut moved = Vec::new();
    let mut still_pending = Vec::new();

    for item in pending.drain(..) {
        if !item.source_path.exists() {
            if item.destination_path.exists() {
                moved.push(item.destination_path);
                continue;
            }

            warn!(
                source = %item.source_path.display(),
                destination = %item.destination_path.display(),
                "Skipping pending archive retry because source file no longer exists"
            );
            still_pending.push(item);
            continue;
        }

        match move_file_to_archive(&item.source_path, &item.destination_path) {
            Ok(()) => moved.push(item.destination_path),
            Err(err) => {
                warn!(
                    source = %item.source_path.display(),
                    destination = %item.destination_path.display(),
                    error = %err,
                    "Archive retry failed; keeping item in pending list"
                );
                still_pending.push(item);
            }
        }
    }

    save_pending_archive_moves(archive_dir, &still_pending)?;

    Ok(ArchiveRetryResult {
        moved,
        pending_count: still_pending.len(),
    })
}

fn build_archive_destination(file_path: &Path, archive_dir: &Path) -> AppResult<PathBuf> {
    fs::create_dir_all(archive_dir).map_err(AppError::IoError)?;

    let filename = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::DomainError("Invalid UTF-8 filename".to_string()))?;

    let archived_name = format!("{}_{}", Local::now().format("%Y%m%d_%H%M%S"), filename);
    Ok(archive_dir.join(archived_name))
}

fn move_file_to_archive(file_path: &Path, archived_path: &Path) -> AppResult<()> {
    if let Some(parent) = archived_path.parent() {
        fs::create_dir_all(parent).map_err(AppError::IoError)?;
    }

    if archived_path.exists() {
        return Err(AppError::DomainError(format!(
            "Archive destination already exists: {}",
            archived_path.display()
        )));
    }

    fs::rename(file_path, archived_path).map_err(AppError::IoError)
}

fn pending_archive_list_path(archive_dir: &Path) -> PathBuf {
    archive_dir.join(ARCHIVE_PENDING_LIST_FILENAME)
}

fn load_pending_archive_moves(archive_dir: &Path) -> AppResult<Vec<PendingArchiveMove>> {
    let path = pending_archive_list_path(archive_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path).map_err(AppError::IoError)?;
    let mut result = Vec::new();

    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Some((source, destination)) = trimmed.split_once('\t') else {
            warn!(
                line_number = line_number + 1,
                line = %trimmed,
                "Skipping malformed archive pending list entry"
            );
            continue;
        };

        result.push(PendingArchiveMove {
            source_path: PathBuf::from(source),
            destination_path: PathBuf::from(destination),
        });
    }

    Ok(result)
}

fn save_pending_archive_moves(archive_dir: &Path, entries: &[PendingArchiveMove]) -> AppResult<()> {
    fs::create_dir_all(archive_dir).map_err(AppError::IoError)?;
    let path = pending_archive_list_path(archive_dir);
    if entries.is_empty() {
        if path.exists() {
            fs::remove_file(path).map_err(AppError::IoError)?;
        }
        return Ok(());
    }

    let mut content = String::new();
    for item in entries {
        content.push_str(&format!(
            "{}\t{}\n",
            item.source_path.display(),
            item.destination_path.display()
        ));
    }
    fs::write(path, content).map_err(AppError::IoError)
}

fn queue_pending_archive_move(
    archive_dir: &Path,
    source_path: &Path,
    destination_path: &Path,
) -> AppResult<()> {
    let mut entries = load_pending_archive_moves(archive_dir)?;
    let already_present = entries.iter().any(|entry| {
        entry.source_path == source_path && entry.destination_path == destination_path
    });
    if !already_present {
        entries.push(PendingArchiveMove {
            source_path: source_path.to_path_buf(),
            destination_path: destination_path.to_path_buf(),
        });
        save_pending_archive_moves(archive_dir, &entries)?;
    }

    Ok(())
}

fn find_required_column_indexes(
    header_row: &[Data],
    sheet_name: &str,
    column_names: &ColumnNames,
) -> Option<ColumnIndexes> {
    let date_visit = find_header_index(header_row, &column_names.date_visit);
    let consume_department = find_header_index(header_row, &column_names.consume_department);
    let code = find_header_index(header_row, &column_names.code);
    let qty = find_header_index(header_row, &column_names.qty);

    let mut missing = Vec::new();
    if date_visit.is_none() {
        missing.push(column_names.date_visit.as_str());
    }
    if consume_department.is_none() {
        missing.push(column_names.consume_department.as_str());
    }
    if code.is_none() {
        missing.push(column_names.code.as_str());
    }
    if qty.is_none() {
        missing.push(column_names.qty.as_str());
    }

    if !missing.is_empty() {
        warn!(
            sheet = %sheet_name,
            missing = %missing.join(", "),
            "Sheet is missing required columns and will be skipped"
        );
        return None;
    }

    let indexes = ColumnIndexes {
        date_visit: date_visit.expect("checked above"),
        consume_department: consume_department.expect("checked above"),
        code: code.expect("checked above"),
        qty: qty.expect("checked above"),
    };

    let positional_warnings = [
        (
            indexes.date_visit,
            DATE_VISIT_COL_IDX,
            column_names.date_visit.as_str(),
        ),
        (
            indexes.consume_department,
            CONSUME_DEPARTMENT_COL_IDX,
            column_names.consume_department.as_str(),
        ),
        (indexes.code, CODE_COL_IDX, column_names.code.as_str()),
        (indexes.qty, QTY_COL_IDX, column_names.qty.as_str()),
    ];
    for (actual, expected, name) in positional_warnings {
        if actual != expected {
            warn!(
                sheet = %sheet_name,
                column = %name,
                expected_index = expected,
                actual_index = actual,
                "Required column is not in fixed position"
            );
        }
    }

    Some(indexes)
}

fn find_header_index(header_row: &[Data], expected: &str) -> Option<usize> {
    header_row.iter().enumerate().find_map(|(index, cell)| {
        cell_as_string(cell)
            .map(|value| value.trim().eq_ignore_ascii_case(expected))
            .unwrap_or(false)
            .then_some(index)
    })
}

fn row_is_empty(row: &[Data]) -> bool {
    row.iter().all(|cell| match cell {
        Data::Empty => true,
        Data::String(s) => s.trim().is_empty(),
        _ => false,
    })
}

fn cell_as_string(cell: &Data) -> Option<String> {
    match cell {
        Data::String(s) => Some(s.trim().to_string()),
        Data::Float(f) => Some(f.to_string()),
        Data::Int(i) => Some(i.to_string()),
        Data::Bool(v) => Some(v.to_string()),
        Data::DateTime(dt) => Some(dt.to_string()),
        Data::DateTimeIso(s) => Some(s.trim().to_string()),
        Data::DurationIso(s) => Some(s.trim().to_string()),
        Data::Error(_) | Data::Empty => None,
    }
}

fn parse_decimal(cell: &Data) -> Option<Decimal> {
    match cell {
        Data::Int(i) => Some(Decimal::from(*i)),
        Data::Float(f) => Decimal::from_f64_retain(*f),
        Data::String(s) => s.trim().replace(',', "").parse::<Decimal>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColumnNames, Config, ProductConfig, Settings};
    use crate::db::Database;
    use crate::models::{FileHistory, LedgerEntry};
    use crate::repository::Repository;
    use chrono::TimeZone;
    use rust_xlsxwriter::Workbook;
    use std::collections::BTreeMap;
    use std::env;
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    #[test]
    fn euclidean_mod_normalizes_negative_dividend() {
        let result = euclidean_mod(Decimal::new(-1, 0), Decimal::new(5, 0))
            .expect("euclidean modulo should succeed for positive divisor");

        assert_eq!(result, Decimal::new(4, 0));
    }

    #[test]
    fn euclidean_mod_rejects_non_positive_divisor() {
        let zero_divisor = euclidean_mod(Decimal::new(1, 0), Decimal::ZERO);
        assert!(matches!(
            zero_divisor,
            Err(AppError::DomainError(message)) if message == "Euclidean modulo divisor must be > 0"
        ));

        let negative_divisor = euclidean_mod(Decimal::new(1, 0), Decimal::new(-2, 0));
        assert!(matches!(
            negative_divisor,
            Err(AppError::DomainError(message)) if message == "Euclidean modulo divisor must be > 0"
        ));
    }

    #[test]
    fn end_to_end_ingestion_validates_success_and_rejections() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
                ("ER".to_string(), "Emergency".to_string()),
                ("ICU".to_string(), "Intensive Care".to_string()),
            ]),
        };

        let first_file = input_dir.join("first.xlsx");
        write_excel(
            &first_file,
            &[
                RowInput {
                    date_visit_be: "01-04-2569 08:00",
                    department: "ER",
                    code: "P001",
                    qty: 3,
                },
                RowInput {
                    date_visit_be: "01-04-2569 09:00",
                    department: "ICU",
                    code: "P001",
                    qty: 2,
                },
            ],
        );

        let first = ingest_excel_file(&first_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("first file should ingest successfully");
        assert!(first.report_path.exists());
        assert!(first
            .archived_path
            .as_ref()
            .expect("archive should succeed")
            .exists());
        assert!(!first_file.exists());
        assert!(repo
            .get_file_history_by_hash(&first.file_hash)
            .expect("file history query should work")
            .is_some());
        let first_history = repo
            .get_file_history_by_hash(&first.file_hash)
            .expect("file history query should work")
            .expect("file history should be present");
        assert_eq!(first_history.filename, "first.xlsx");
        assert!(first_history.file_size > 0);
        assert_eq!(
            first_history.transaction_date,
            Utc.with_ymd_and_hms(2026, 4, 1, 8, 0, 0)
                .single()
                .expect("valid timestamp")
        );
        assert_eq!(
            repo.get_ledger_entries_by_file_hash(&first.file_hash)
                .expect("ledger query should work")
                .len(),
            2
        );
        assert_eq!(
            repo.get_total("P001").expect("total query should work"),
            Decimal::new(5, 0)
        );
        assert_eq!(
            fs::read_dir(&reports_dir)
                .expect("reports dir should exist")
                .count(),
            1
        );
        assert_eq!(
            fs::read_dir(&archive_dir)
                .expect("archive dir should exist")
                .count(),
            1
        );

        thread::sleep(Duration::from_secs(1));

        let second_file = input_dir.join("second.xlsx");
        write_excel(
            &second_file,
            &[RowInput {
                date_visit_be: "02-04-2569 10:00",
                department: "ER",
                code: "P001",
                qty: 4,
            }],
        );

        let second = ingest_excel_file(&second_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("second later file should ingest successfully");
        assert!(second.report_path.exists());
        assert!(second
            .archived_path
            .as_ref()
            .expect("archive should succeed")
            .exists());
        assert_eq!(
            repo.get_total("P001").expect("total query should work"),
            Decimal::new(9, 0)
        );
        assert_eq!(
            fs::read_dir(&reports_dir)
                .expect("reports dir should exist")
                .count(),
            2
        );

        let older_file = input_dir.join("older.xlsx");
        write_excel(
            &older_file,
            &[RowInput {
                date_visit_be: "01-04-2569 07:00",
                department: "ER",
                code: "P001",
                qty: 1,
            }],
        );

        let older_result =
            ingest_excel_file(&older_file, &config, &repo, &archive_dir, &reports_dir);
        assert!(matches!(
            older_result,
            Err(AppError::ChronologicalViolation { .. })
        ));

        let duplicate_file = input_dir.join("duplicate.xlsx");
        fs::copy(
            second
                .archived_path
                .as_ref()
                .expect("archive should succeed"),
            &duplicate_file,
        )
        .expect("should copy archived file");

        let duplicate_result =
            ingest_excel_file(&duplicate_file, &config, &repo, &archive_dir, &reports_dir);
        assert!(matches!(
            duplicate_result,
            Err(AppError::DomainError(message)) if message.contains("already been processed")
        ));
    }

    #[test]
    fn duplicate_detection_uses_hash_across_different_paths() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let source = input_dir.join("source.xlsx");
        write_excel(
            &source,
            &[RowInput {
                date_visit_be: "01-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 2,
            }],
        );

        let outcome = ingest_excel_file(&source, &config, &repo, &archive_dir, &reports_dir)
            .expect("initial file should ingest");
        let duplicate_other_path = input_dir.join("renamed_copy.xlsx");
        fs::copy(
            outcome
                .archived_path
                .as_ref()
                .expect("archive should succeed"),
            &duplicate_other_path,
        )
        .expect("duplicate should be copied into input directory");

        let duplicate = ingest_excel_file(
            &duplicate_other_path,
            &config,
            &repo,
            &archive_dir,
            &reports_dir,
        );
        assert!(matches!(
            duplicate,
            Err(AppError::DomainError(message)) if message.contains("already been processed")
        ));
    }

    #[test]
    fn happy_path_persists_file_hash_and_metadata() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let first_file = input_dir.join("persist_me.xlsx");
        write_excel(
            &first_file,
            &[RowInput {
                date_visit_be: "01-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 5,
            }],
        );

        let expected_hash = compute_file_hash(&first_file).expect("hash should be computed");
        let expected_size = i64::try_from(
            fs::metadata(&first_file)
                .expect("metadata should load")
                .len(),
        )
        .expect("test file size should fit in i64");

        let outcome = ingest_excel_file(&first_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("file should ingest successfully");

        assert_eq!(outcome.file_hash, expected_hash);
        let history = repo
            .get_file_history_by_hash(&outcome.file_hash)
            .expect("query should succeed")
            .expect("file history row should exist");

        assert_eq!(history.file_hash, expected_hash);
        assert_eq!(history.filename, "persist_me.xlsx");
        assert_eq!(history.file_size, expected_size);
        assert_eq!(
            history.transaction_date,
            Utc.with_ymd_and_hms(2026, 4, 1, 8, 0, 0)
                .single()
                .expect("valid timestamp")
        );
        assert!(repo
            .exists_by_hash(&expected_hash)
            .expect("duplicate query should succeed"));
    }

    #[test]
    fn dry_run_phase_does_not_write_to_database_before_confirm() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let file = input_dir.join("dry_run_only.xlsx");
        write_excel(
            &file,
            &[RowInput {
                date_visit_be: "01-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 5,
            }],
        );

        let pending = prepare_ingestion_dry_run(&file, &config, &repo)
            .expect("dry-run preparation should succeed");
        assert_eq!(pending.dry_run_rows.len(), 1);

        assert!(
            !repo
                .exists_by_hash(&pending.file_hash)
                .expect("file_history query should succeed"),
            "dry-run phase must not write file_history"
        );
        assert_eq!(
            repo.get_ledger_entries_by_file_hash(&pending.file_hash)
                .expect("ledger query should succeed")
                .len(),
            0,
            "dry-run phase must not write inventory_ledger"
        );
        assert_eq!(
            repo.get_total("P001").expect("total query should succeed"),
            Decimal::ZERO,
            "dry-run phase must not write product_totals"
        );
    }

    #[test]
    fn confirm_phase_commits_prepared_ingestion() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let file = input_dir.join("confirm_commit.xlsx");
        write_excel(
            &file,
            &[RowInput {
                date_visit_be: "01-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 5,
            }],
        );

        let pending = prepare_ingestion_dry_run(&file, &config, &repo)
            .expect("dry-run preparation should succeed");
        let outcome = commit_prepared_ingestion(&pending, &repo, &reports_dir, &archive_dir)
            .expect("confirm phase should commit successfully");

        assert!(outcome.report_path.exists());
        assert!(outcome
            .archived_path
            .as_ref()
            .expect("archive should succeed")
            .exists());
        assert!(!outcome.archive_move_pending);
        assert!(repo
            .exists_by_hash(&pending.file_hash)
            .expect("file_history query should succeed"));
        assert_eq!(
            repo.get_ledger_entries_by_file_hash(&pending.file_hash)
                .expect("ledger query should succeed")
                .len(),
            1
        );
        assert_eq!(
            repo.get_total("P001").expect("total query should succeed"),
            Decimal::new(5, 0)
        );
    }

    #[test]
    fn archive_move_uses_required_timestamp_prefix_format() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let file = input_dir.join("naming_check.xlsx");
        write_excel(
            &file,
            &[RowInput {
                date_visit_be: "01-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 1,
            }],
        );

        let outcome = ingest_excel_file(&file, &config, &repo, &archive_dir, &reports_dir)
            .expect("ingestion should succeed");
        let archived = outcome
            .archived_path
            .as_ref()
            .expect("archive should succeed");

        let archived_name = archived
            .file_name()
            .and_then(|value| value.to_str())
            .expect("archive filename should be valid UTF-8");

        assert!(archived_name.ends_with("_naming_check.xlsx"));
        assert_eq!(archived_name.as_bytes()[8], b'_');
        assert_eq!(archived_name.as_bytes()[15], b'_');
        assert!(archived_name[..8].chars().all(|ch| ch.is_ascii_digit()));
        assert!(archived_name[9..15].chars().all(|ch| ch.is_ascii_digit()));
    }

    #[test]
    fn archive_move_failure_queues_retry_and_manual_retry_moves_file() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let file = input_dir.join("retry_case.xlsx");
        write_excel(
            &file,
            &[RowInput {
                date_visit_be: "01-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 1,
            }],
        );

        let pending = prepare_ingestion_dry_run(&file, &config, &repo)
            .expect("dry-run preparation should succeed");

        fs::remove_file(&file).expect("source file should be deleted to simulate move failure");

        let outcome = commit_prepared_ingestion(&pending, &repo, &reports_dir, &archive_dir)
            .expect("commit should succeed even when archive move fails");

        assert!(outcome.report_path.exists());
        assert!(outcome.archived_path.is_none());
        assert!(outcome.archive_move_pending);

        assert!(repo
            .exists_by_hash(&pending.file_hash)
            .expect("file_history should exist after commit"));

        let pending_file_path = archive_dir.join(ARCHIVE_PENDING_LIST_FILENAME);
        let pending_content =
            fs::read_to_string(&pending_file_path).expect("pending archive list should be created");
        assert!(pending_content.contains(file.to_string_lossy().as_ref()));

        fs::write(&file, b"recreated file for retry").expect("source file should be recreated");

        let retry_result =
            retry_pending_archive_moves(&archive_dir).expect("retry operation should succeed");
        assert_eq!(retry_result.moved.len(), 1);
        assert_eq!(retry_result.pending_count, 0);
        assert!(!file.exists());
        assert!(retry_result.moved[0].exists());
        assert!(
            !pending_file_path.exists(),
            "pending list should be removed after successful retry"
        );
    }

    #[test]
    fn ingestion_performance_guard_for_500_rows_with_50k_existing_rows() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
                ("ER".to_string(), "Emergency".to_string()),
                ("ICU".to_string(), "Intensive Care".to_string()),
            ]),
        };

        // Pre-seed realistic history: 50k existing ledger rows.
        let seed_hash = "seed-file-50k".to_string();
        let seed_date = NaiveDate::from_ymd_opt(2026, 4, 1).expect("valid seed date");
        let seed_file = FileHistory {
            file_hash: seed_hash.clone(),
            filename: "seed.xlsx".to_string(),
            file_size: 0,
            transaction_date: seed_date
                .and_hms_opt(0, 0, 0)
                .expect("valid seed datetime")
                .and_utc(),
        };

        let mut seed_entries = Vec::with_capacity(50_000);
        for i in 0..50_000 {
            seed_entries.push(LedgerEntry {
                product_id: "P001".to_string(),
                department_id: if i % 2 == 0 {
                    "ER".to_string()
                } else {
                    "ICU".to_string()
                },
                dispensed_amount: Decimal::ONE,
                transaction_date: seed_date
                    .and_hms_opt(0, 0, 0)
                    .expect("valid seed datetime")
                    .and_utc(),
                file_hash: seed_hash.clone(),
            });
        }
        repo.commit_ingestion_batch(&seed_file, &seed_entries)
            .expect("seed commit should succeed");

        // New workbook with 500 rows on a strictly newer date.
        let perf_file = input_dir.join("perf_500.xlsx");
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("P001").expect("sheet name should be set");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Date Visit")
            .expect("header should be written");
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Consume Department")
            .expect("header should be written");
        sheet
            .write_string(0, CODE_COL_IDX as u16, "Code")
            .expect("header should be written");
        sheet
            .write_string(0, QTY_COL_IDX as u16, "Qty")
            .expect("header should be written");

        for idx in 0..500_u32 {
            let row = idx + 1;
            let minute = idx % 60;
            let date_visit = format!("02-04-2569 08:{minute:02}");
            let department = if idx % 2 == 0 { "ER" } else { "ICU" };
            sheet
                .write_string(row, DATE_VISIT_COL_IDX as u16, &date_visit)
                .expect("date should be written");
            sheet
                .write_string(row, CONSUME_DEPARTMENT_COL_IDX as u16, department)
                .expect("department should be written");
            sheet
                .write_string(row, CODE_COL_IDX as u16, "P001")
                .expect("code should be written");
            sheet
                .write_number(row, QTY_COL_IDX as u16, 1.0)
                .expect("qty should be written");
        }
        workbook
            .save(&perf_file)
            .expect("performance workbook should be saved");

        let started = Instant::now();
        let outcome = ingest_excel_file(&perf_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("performance ingestion should succeed");
        let elapsed = started.elapsed();

        assert!(outcome.report_path.exists());
        assert!(outcome
            .archived_path
            .as_ref()
            .expect("archive should succeed")
            .exists());
        assert_eq!(
            repo.get_ledger_entries_by_file_hash(&outcome.file_hash)
                .expect("ledger query should work")
                .len(),
            500
        );

        // Hardware-adaptive guard for CI plus strict opt-in SLA enforcement.
        // - Strict mode: 2s hard requirement (for representative office hardware runners).
        // - Default mode: adaptive budget derived from parse-only time on this machine,
        //   capped at 6s to still catch meaningful regressions in slower CI.
        let parse_started = Instant::now();
        let parsed = parse_excel_file(
            outcome
                .archived_path
                .as_ref()
                .expect("archive should succeed"),
            &config,
        )
        .expect("parse-only calibration should succeed");
        let parse_elapsed = parse_started.elapsed();
        assert_eq!(parsed.rows.len(), 500);

        let strict_budget = Duration::from_secs(2);
        let adaptive_ms = (parse_elapsed.as_millis() as u64)
            .saturating_mul(8)
            .saturating_add(1_200);
        let adaptive_budget = Duration::from_millis(adaptive_ms.clamp(2_000, 6_000));

        let strict_mode = env::var("NETHERICA_ENFORCE_STRICT_PERF")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
            .unwrap_or(false);
        let allowed_budget = if strict_mode {
            strict_budget
        } else {
            adaptive_budget
        };

        println!(
            "ingestion_perf: elapsed={elapsed:?}, parse_only={parse_elapsed:?}, allowed={allowed_budget:?}, strict_mode={strict_mode}"
        );

        assert!(
            elapsed <= allowed_budget,
            "500-row ingestion exceeded budget: elapsed={elapsed:?}, allowed={allowed_budget:?}, parse_only={parse_elapsed:?}. \
             Set NETHERICA_ENFORCE_STRICT_PERF=1 to enforce 2s SLA exactly."
        );
    }

    struct RowInput<'a> {
        date_visit_be: &'a str,
        department: &'a str,
        code: &'a str,
        qty: i64,
    }

    fn write_excel(path: &Path, rows: &[RowInput<'_>]) {
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name("P001")
            .expect("sheet name should be set");

        worksheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Date Visit")
            .expect("header should be written");
        worksheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Consume Department")
            .expect("header should be written");
        worksheet
            .write_string(0, CODE_COL_IDX as u16, "Code")
            .expect("header should be written");
        worksheet
            .write_string(0, QTY_COL_IDX as u16, "Qty")
            .expect("header should be written");

        for (idx, row) in rows.iter().enumerate() {
            let r = (idx + 1) as u32;
            worksheet
                .write_string(r, DATE_VISIT_COL_IDX as u16, row.date_visit_be)
                .expect("date should be written");
            worksheet
                .write_string(r, CONSUME_DEPARTMENT_COL_IDX as u16, row.department)
                .expect("department should be written");
            worksheet
                .write_string(r, CODE_COL_IDX as u16, row.code)
                .expect("code should be written");
            worksheet
                .write_number(r, QTY_COL_IDX as u16, row.qty as f64)
                .expect("qty should be written");
        }

        workbook.save(path).expect("workbook should be saved");
    }

    #[test]
    fn converts_be_datetime_to_gregorian_utc() {
        let dt = be_to_gregorian("08-04-2569 13:45").expect("BE datetime should parse");
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 4);
        assert_eq!(dt.day(), 8);
        assert_eq!(gregorian_to_be(dt), "08-04-2569 13:45");
    }

    #[test]
    fn parse_excel_file_skips_invalid_rows_and_keeps_valid_rows() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("rows_validation.xlsx");

        let config = Config {
            database_path: temp.path().join("state.db"),
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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("P001").expect("sheet name should be set");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Date Visit")
            .expect("header should be written");
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Consume Department")
            .expect("header should be written");
        sheet
            .write_string(0, CODE_COL_IDX as u16, "Code")
            .expect("header should be written");
        sheet
            .write_string(0, QTY_COL_IDX as u16, "Qty")
            .expect("header should be written");

        sheet
            .write_string(1, DATE_VISIT_COL_IDX as u16, "01-04-2569 08:00")
            .expect("date should be written");
        sheet
            .write_string(1, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(1, CODE_COL_IDX as u16, "P999")
            .expect("code should be written");
        sheet
            .write_number(1, QTY_COL_IDX as u16, 1.0)
            .expect("qty should be written");

        sheet
            .write_string(2, DATE_VISIT_COL_IDX as u16, "01-04-2569 09:00")
            .expect("date should be written");
        sheet
            .write_string(2, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(2, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_string(2, QTY_COL_IDX as u16, "abc")
            .expect("qty should be written");

        sheet
            .write_string(3, DATE_VISIT_COL_IDX as u16, "01-04-2569 10:00")
            .expect("date should be written");
        sheet
            .write_string(3, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(3, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_string(3, QTY_COL_IDX as u16, "1,234.50")
            .expect("qty should be written");

        sheet
            .write_string(4, DATE_VISIT_COL_IDX as u16, "01-04-2569 11:00")
            .expect("date should be written");
        sheet
            .write_string(4, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(4, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_number(4, QTY_COL_IDX as u16, 0.0)
            .expect("qty should be written");

        sheet
            .write_string(5, DATE_VISIT_COL_IDX as u16, "32-13-2569 12:00")
            .expect("date should be written");
        sheet
            .write_string(5, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(5, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_number(5, QTY_COL_IDX as u16, 2.0)
            .expect("qty should be written");

        workbook
            .save(&workbook_path)
            .expect("workbook should be saved");

        let parsed = parse_excel_file(&workbook_path, &config).expect("parse should succeed");
        assert_eq!(parsed.rows.len(), 2);
        assert_eq!(parsed.rows[0].product_id, "P001");
        assert_eq!(parsed.rows[0].department_id, "ER");
        assert_eq!(parsed.rows[0].dispensed_amount, Decimal::new(123450, 2));
        assert_eq!(parsed.rows[1].dispensed_amount, Decimal::new(2, 0));
        assert!(parsed.transaction_date_fallback_used);
        assert!(parsed.transaction_date_warning.is_some());
    }

    #[test]
    fn parse_excel_file_falls_back_to_file_mtime_when_dates_are_missing_or_invalid() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("fallback_dates.xlsx");

        let config = Config {
            database_path: temp.path().join("state.db"),
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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("P001").expect("sheet name should be set");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Date Visit")
            .expect("header should be written");
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Consume Department")
            .expect("header should be written");
        sheet
            .write_string(0, CODE_COL_IDX as u16, "Code")
            .expect("header should be written");
        sheet
            .write_string(0, QTY_COL_IDX as u16, "Qty")
            .expect("header should be written");

        sheet
            .write_string(1, DATE_VISIT_COL_IDX as u16, "")
            .expect("date should be written");
        sheet
            .write_string(1, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(1, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_number(1, QTY_COL_IDX as u16, 1.0)
            .expect("qty should be written");

        sheet
            .write_string(2, DATE_VISIT_COL_IDX as u16, "99-99-9999 99:99")
            .expect("date should be written");
        sheet
            .write_string(2, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(2, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_number(2, QTY_COL_IDX as u16, 2.0)
            .expect("qty should be written");

        workbook
            .save(&workbook_path)
            .expect("workbook should be saved");

        let expected_mtime = file_modified_time_utc(&workbook_path).expect("mtime should load");
        let parsed = parse_excel_file(&workbook_path, &config).expect("parse should succeed");

        assert_eq!(parsed.rows.len(), 2);
        assert!(parsed
            .rows
            .iter()
            .all(|row| row.transaction_date == expected_mtime));
        assert_eq!(parsed.earliest_transaction_utc, expected_mtime);
        assert!(parsed.transaction_date_fallback_used);
        let warning = parsed
            .transaction_date_warning
            .expect("warning text should be present");
        assert!(warning.contains("Using file modification time (UTC)"));
    }

    #[test]
    fn chronology_check_uses_fallback_transaction_date_when_dates_invalid() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let existing_history = FileHistory {
            file_hash: "existing".to_string(),
            filename: "existing.xlsx".to_string(),
            file_size: 1,
            transaction_date: Utc
                .with_ymd_and_hms(2100, 1, 1, 0, 0, 0)
                .single()
                .expect("valid date"),
        };
        repo.commit_ingestion_batch(&existing_history, &[])
            .expect("seed history should insert");

        let file = input_dir.join("invalid_date_rows.xlsx");
        write_excel(
            &file,
            &[RowInput {
                date_visit_be: "invalid-date",
                department: "ER",
                code: "P001",
                qty: 1,
            }],
        );

        let pending = prepare_ingestion_dry_run(&file, &config, &repo);
        assert!(matches!(
            pending,
            Err(AppError::ChronologicalViolation { .. })
        ));
    }

    #[test]
    fn parse_excel_file_fails_when_required_columns_missing_on_all_matching_sheets() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("missing_columns.xlsx");

        let config = Config {
            database_path: temp.path().join("state.db"),
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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("P001").expect("sheet name should be set");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Date Visit")
            .expect("header should be written");
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Department")
            .expect("header should be written");
        sheet
            .write_string(0, CODE_COL_IDX as u16, "Code")
            .expect("header should be written");
        sheet
            .write_string(0, QTY_COL_IDX as u16, "Quantity")
            .expect("header should be written");

        workbook
            .save(&workbook_path)
            .expect("workbook should be saved");

        let err = match parse_excel_file(&workbook_path, &config) {
            Ok(_) => panic!("parse should fail"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            AppError::ExcelError(message) if message == "Required column(s) not found."
        ));
    }

    #[test]
    fn parse_excel_file_accepts_case_insensitive_trimmed_required_headers() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("header_case_trim.xlsx");

        let config = Config {
            database_path: temp.path().join("state.db"),
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
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("P001").expect("sheet name should be set");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "  date visit ")
            .expect("header should be written");
        sheet
            .write_string(
                0,
                CONSUME_DEPARTMENT_COL_IDX as u16,
                "  CONSUME DEPARTMENT  ",
            )
            .expect("header should be written");
        sheet
            .write_string(0, CODE_COL_IDX as u16, " code ")
            .expect("header should be written");
        sheet
            .write_string(0, QTY_COL_IDX as u16, " qty ")
            .expect("header should be written");

        sheet
            .write_string(1, DATE_VISIT_COL_IDX as u16, "01-04-2569 08:00")
            .expect("date should be written");
        sheet
            .write_string(1, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(1, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_number(1, QTY_COL_IDX as u16, 2.0)
            .expect("qty should be written");

        workbook
            .save(&workbook_path)
            .expect("workbook should be saved");

        let parsed = parse_excel_file(&workbook_path, &config).expect("parse should succeed");
        assert_eq!(parsed.rows.len(), 1);
    }

    #[test]
    fn parse_excel_file_respects_custom_column_names_from_config() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("custom_headers.xlsx");

        let config = Config {
            database_path: temp.path().join("state.db"),
            settings: Settings {
                strict_chronological: true,
            },
            column_names: ColumnNames {
                date_visit: "Visit Date".to_string(),
                consume_department: "Department Used".to_string(),
                code: "Item Code".to_string(),
                qty: "Amount".to_string(),
            },
            products: vec![ProductConfig {
                id: "P001".to_string(),
                display_name: "Product 001".to_string(),
                unit: "Box".to_string(),
                subunit: "Piece".to_string(),
                factor: Decimal::new(2, 0),
                track_subunits: true,
            }],
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("P001").expect("sheet name should be set");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Visit Date")
            .expect("header should be written");
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Department Used")
            .expect("header should be written");
        sheet
            .write_string(0, CODE_COL_IDX as u16, "Item Code")
            .expect("header should be written");
        sheet
            .write_string(0, QTY_COL_IDX as u16, "Amount")
            .expect("header should be written");

        sheet
            .write_string(1, DATE_VISIT_COL_IDX as u16, "01-04-2569 08:00")
            .expect("date should be written");
        sheet
            .write_string(1, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .expect("department should be written");
        sheet
            .write_string(1, CODE_COL_IDX as u16, "P001")
            .expect("code should be written");
        sheet
            .write_number(1, QTY_COL_IDX as u16, 2.0)
            .expect("qty should be written");

        workbook
            .save(&workbook_path)
            .expect("workbook should be saved");

        let parsed = parse_excel_file(&workbook_path, &config).expect("parse should succeed");
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].product_id, "P001");
        assert_eq!(parsed.rows[0].department_id, "ER");
        assert_eq!(parsed.rows[0].dispensed_amount, Decimal::new(2, 0));
    }
}
