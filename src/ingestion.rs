use crate::config::{ColumnNames, Config};
use crate::domain::DryRunRow;
use crate::error::{AppError, AppResult};
use crate::models::{FileHistory, LedgerEntry, LedgerRow};
use crate::report::{self, ReportRenderInput};
use crate::repository::Repository;
use calamine::{open_workbook_auto, Data, DataType, Reader};
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

    if let Some(existing_max) = repository.get_max_ledger_transaction_date()? {
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
            borrowed_amount: Decimal::ZERO,
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
    config: &Config,
    repository: &Repository<'_>,
    reports_dir: &Path,
    archive_dir: &Path,
) -> AppResult<IngestionOutcome> {
    let file_history = FileHistory {
        file_hash: pending.file_hash.clone(),
        filename: pending.filename.clone(),
        file_size: pending.file_size,
        transaction_date: pending.transaction_date,
        period_end: pending.period_end,
    };

    repository.commit_ingestion_batch(&file_history, &pending.ledger_entries)?;

    let report_rows = report::build_report_rows_for_entries(
        repository,
        config,
        &pending.ledger_entries,
        pending.period_start,
    )?;

    let carryover_updates = build_borrowed_carryover_updates(&report_rows);
    repository.upsert_borrowed_carryover_batch(&carryover_updates)?;

    let report_input = ReportRenderInput {
        source_filename: pending.filename.clone(),
        file_hash: pending.file_hash.clone(),
        generated_at_utc: Utc::now(),
        period_start_utc: pending.period_start,
        period_end_utc: pending.period_end,
        rows: report_rows,
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
    commit_prepared_ingestion(&pending, config, repository, reports_dir, archive_dir)
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
                    subunit: product.subunit.clone(),
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
    let filename_date_range = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(parse_filename_date_range);
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
    let mut date_sources = Vec::<DateSource>::new();
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

            let cell = row.get(column_indexes.date_visit).unwrap_or(&Data::Empty);
            let transaction_date_result = extract_transaction_date(cell, file_mtime_utc);
            let transaction_date = transaction_date_result.date();
            let date_source = transaction_date_result
                .source()
                .unwrap_or(DateSource::StringSource);

            match transaction_date_result {
                DateExtractionResult::Extracted(_, _) => {
                    tracing::debug!(sheet = %product.id, row_number = row_idx + 2, "Native date extracted");
                }
                DateExtractionResult::FallbackMissing(_) => {
                    fallback_rows += 1;
                    warn!(
                        sheet = %product.id,
                        row_number = row_idx + 2,
                        fallback_utc = %file_mtime_utc,
                        "Date missing; using file modification timestamp as UTC fallback"
                    );
                }
                DateExtractionResult::FallbackUnparseable(_) => {
                    fallback_rows += 1;
                    warn!(
                        sheet = %product.id,
                        row_number = row_idx + 2,
                        fallback_utc = %file_mtime_utc,
                        "Date unparseable; using file modification timestamp as UTC fallback"
                    );
                }
            }

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
            date_sources.push(date_source);
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

    if let Some(range) = filename_date_range {
        let repaired = repair_excel_corrupted_dates(&mut rows, &date_sources, range);
        if repaired > 0 {
            earliest = rows.iter().map(|r| r.transaction_date).min();
            tracing::info!(
                repaired_count = repaired,
                "Repaired Excel-corrupted DD/MM swapped dates using filename anchor"
            );
        }
    }

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

pub fn parse_filename_date_range(filename: &str) -> Option<(NaiveDate, NaiveDate)> {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    let open = stem.find('(')?;
    let close = stem.rfind(')')?;
    if close <= open {
        return None;
    }
    let inner = &stem[open + 1..close];
    let parts: Vec<&str> = inner.split(" - ").collect();
    if parts.len() != 2 {
        return None;
    }

    let start = parse_english_date(parts[0].trim())?;
    let end = parse_english_date(parts[1].trim())?;
    Some((start, end))
}

fn parse_english_date(input: &str) -> Option<NaiveDate> {
    let input = input.trim();
    let (day_str, rest) = split_first_space(input)?;
    let day: u32 = day_str.parse().ok()?;
    let (month_str, year_str) = split_first_space(rest)?;
    let month = month_abbreviation_to_number(month_str)?;
    let year: i32 = year_str.trim_end_matches('.').parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

fn split_first_space(s: &str) -> Option<(&str, &str)> {
    let idx = s.find(' ')?;
    Some((&s[..idx], &s[idx + 1..]))
}

fn month_abbreviation_to_number(abbr: &str) -> Option<u32> {
    Some(match abbr {
        "Jan." => 1,
        "Feb." => 2,
        "Mar." => 3,
        "Apr." => 4,
        "May." => 5,
        "Jun." => 6,
        "Jul." => 7,
        "Aug." => 8,
        "Sep." => 9,
        "Oct." => 10,
        "Nov." => 11,
        "Dec." => 12,
        _ => return None,
    })
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

fn repair_excel_corrupted_dates(
    rows: &mut [LedgerRow],
    sources: &[DateSource],
    range: (NaiveDate, NaiveDate),
) -> usize {
    let (range_start, range_end) = range;
    let mut repaired = 0usize;

    for (row, source) in rows.iter_mut().zip(sources.iter()) {
        if *source != DateSource::ExcelNative {
            continue;
        }

        let naive_date = row.transaction_date.date_naive();
        if naive_date >= range_start && naive_date <= range_end {
            continue;
        }

        let day = naive_date.day();
        let month = naive_date.month();
        if day > 12 {
            continue;
        }

        if let Some(swapped) = NaiveDate::from_ymd_opt(naive_date.year(), day, month) {
            if swapped >= range_start && swapped <= range_end {
                let time = row.transaction_date.time();
                if let Some(new_naive) =
                    swapped.and_hms_opt(time.hour(), time.minute(), time.second())
                {
                    row.transaction_date =
                        DateTime::<Utc>::from_naive_utc_and_offset(new_naive, Utc);
                    repaired += 1;
                }
            }
        }
    }

    repaired
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSource {
    ExcelNative,
    StringSource,
}

#[derive(Debug, PartialEq)]
pub enum DateExtractionResult {
    Extracted(DateTime<Utc>, DateSource),
    FallbackMissing(DateTime<Utc>),
    FallbackUnparseable(DateTime<Utc>),
}

impl DateExtractionResult {
    pub fn date(&self) -> DateTime<Utc> {
        match self {
            Self::Extracted(dt, _) => *dt,
            Self::FallbackMissing(dt) => *dt,
            Self::FallbackUnparseable(dt) => *dt,
        }
    }

    pub fn is_fallback(&self) -> bool {
        matches!(
            self,
            Self::FallbackMissing(_) | Self::FallbackUnparseable(_)
        )
    }

    pub fn source(&self) -> Option<DateSource> {
        match self {
            Self::Extracted(_, s) => Some(*s),
            _ => None,
        }
    }
}

fn apply_be_heuristic(naive: NaiveDateTime) -> DateTime<Utc> {
    let mut dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
    if dt.year() > 2400 {
        let new_year = dt.year() - 543;
        if let Some(new_date) = NaiveDate::from_ymd_opt(new_year, dt.month(), dt.day()) {
            if let Some(new_naive) = new_date.and_hms_opt(dt.hour(), dt.minute(), dt.second()) {
                let adjusted = DateTime::<Utc>::from_naive_utc_and_offset(new_naive, Utc);
                tracing::warn!(
                    original_year = dt.year(),
                    adjusted_year = adjusted.year(),
                    "Native Date formatted cell contained a Buddhist Era year. Applied -543 heuristic correction."
                );
                dt = adjusted;
            }
        }
    }
    dt
}

pub fn extract_transaction_date(cell: &Data, file_mtime: DateTime<Utc>) -> DateExtractionResult {
    match cell {
        Data::Empty => return DateExtractionResult::FallbackMissing(file_mtime),
        Data::DateTime(excel_dt) => {
            if let Some(naive) = excel_dt.as_datetime() {
                return DateExtractionResult::Extracted(
                    apply_be_heuristic(naive),
                    DateSource::ExcelNative,
                );
            }
        }
        Data::DateTimeIso(iso_str) => {
            if let Ok(naive) = NaiveDateTime::parse_from_str(iso_str.trim(), "%Y-%m-%dT%H:%M:%S") {
                return DateExtractionResult::Extracted(
                    apply_be_heuristic(naive),
                    DateSource::ExcelNative,
                );
            }
        }
        Data::Float(serial) => {
            let dt =
                calamine::ExcelDateTime::new(*serial, calamine::ExcelDateTimeType::DateTime, false);
            if let Some(naive) = dt.as_datetime() {
                return DateExtractionResult::Extracted(
                    apply_be_heuristic(naive),
                    DateSource::ExcelNative,
                );
            }
        }
        Data::String(s) => {
            if let Ok(dt) = be_to_gregorian(s) {
                return DateExtractionResult::Extracted(dt, DateSource::StringSource);
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M:%S") {
                return DateExtractionResult::Extracted(
                    apply_be_heuristic(naive),
                    DateSource::StringSource,
                );
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S") {
                return DateExtractionResult::Extracted(
                    apply_be_heuristic(naive),
                    DateSource::StringSource,
                );
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M") {
                return DateExtractionResult::Extracted(
                    apply_be_heuristic(naive),
                    DateSource::StringSource,
                );
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(s.trim(), "%d/%m/%Y %H:%M") {
                return DateExtractionResult::Extracted(
                    apply_be_heuristic(naive),
                    DateSource::StringSource,
                );
            }
        }
        _ => {}
    }

    if let Some(naive) = cell.as_datetime() {
        return DateExtractionResult::Extracted(apply_be_heuristic(naive), DateSource::ExcelNative);
    }

    if let Data::String(_) = cell {
        DateExtractionResult::FallbackUnparseable(file_mtime)
    } else {
        DateExtractionResult::FallbackMissing(file_mtime)
    }
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
    let product_meta_by_id = config
        .products
        .iter()
        .map(|p| (p.id.as_str(), (p.factor, p.display_name.as_str())))
        .collect::<BTreeMap<_, _>>();

    let mut usage_by_product_department = BTreeMap::<(String, String), Decimal>::new();
    let period_start = rows
        .iter()
        .map(|r| r.transaction_date)
        .min()
        .ok_or_else(|| AppError::DomainError("No rows available for dry-run data".to_string()))?;

    for row in rows {
        if row.dispensed_amount == Decimal::ZERO {
            continue;
        }

        usage_by_product_department
            .entry((row.product_id.clone(), row.department_id.clone()))
            .and_modify(|sum| *sum += row.dispensed_amount)
            .or_insert(row.dispensed_amount);
    }

    let mut result = Vec::new();
    for ((product_id, department_id), total_subunits_used) in usage_by_product_department {
        let (factor, product_display_name) = product_meta_by_id
            .get(product_id.as_str())
            .copied()
            .ok_or_else(|| {
            AppError::DomainError(format!("Missing product factor for '{}'", product_id))
        })?;

        let opening_total = repository.sum_before_date_for_product_department(
            &product_id,
            &department_id,
            period_start,
        )?;

        let (opening_leftover, whole_units_output, closing_leftover) = if factor == Decimal::ONE {
            (Decimal::ZERO, total_subunits_used, Decimal::ZERO)
        } else {
            let opening_leftover = euclidean_mod(opening_total, factor)?;
            let new_total = opening_total + total_subunits_used;
            let whole_units_output =
                (new_total / factor).floor() - (opening_total / factor).floor();
            let closing_leftover = euclidean_mod(new_total, factor)?;
            (opening_leftover, whole_units_output, closing_leftover)
        };

        if factor <= Decimal::ZERO {
            return Err(AppError::DomainError(format!(
                "Invalid factor {} for product '{}'",
                factor, product_id
            )));
        }

        let carry_over_borrowed = repository.get_borrowed_carryover(&product_id, &department_id)?;
        let ingested_borrowed = Decimal::ZERO;
        let net_subunits =
            total_subunits_used + opening_leftover - carry_over_borrowed - ingested_borrowed;
        let issued = (net_subunits / factor).floor();

        let department_display_name = config
            .departments
            .get(&department_id)
            .cloned()
            .unwrap_or_else(|| department_id.clone());

        result.push(DryRunRow {
            product_id,
            product_display_name: product_display_name.to_string(),
            department_id,
            department_display_name,
            opening_leftover,
            borrowed: carry_over_borrowed,
            total_subunits_used,
            issued,
            whole_units_output,
            closing_leftover,
        });
    }

    Ok(result)
}

fn build_borrowed_carryover_updates(rows: &[DryRunRow]) -> Vec<(String, String, Decimal)> {
    rows.iter()
        .map(|row| {
            let ingested_borrowed = Decimal::ZERO;
            let net_subunits =
                row.total_subunits_used + row.opening_leftover - row.borrowed - ingested_borrowed;
            let new_carryover = if net_subunits < Decimal::ZERO {
                -net_subunits
            } else {
                Decimal::ZERO
            };

            (
                row.product_id.clone(),
                row.department_id.clone(),
                new_carryover,
            )
        })
        .collect()
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
    fn build_dry_run_rows_is_scoped_by_product_and_department() {
        let temp = tempdir().expect("tempdir should be created");
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
                factor: Decimal::new(5, 0),
                track_subunits: true,
            }],
            departments: BTreeMap::from([
                ("ER".to_string(), "Emergency".to_string()),
                ("ICU".to_string(), "Intensive Care".to_string()),
            ]),
        };

        repo.commit_ingestion_batch(
            &FileHistory {
                file_hash: "seed".to_string(),
                filename: "seed.xlsx".to_string(),
                file_size: 1,
                transaction_date: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).single().unwrap(),
                period_end: Utc.with_ymd_and_hms(2026, 4, 1, 8, 0, 0).single().unwrap(),
            },
            &[LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ER".to_string(),
                dispensed_amount: Decimal::new(7, 0),
                transaction_date: Utc.with_ymd_and_hms(2026, 4, 1, 8, 0, 0).single().unwrap(),
                file_hash: "seed".to_string(),
                borrowed_amount: Decimal::ZERO,
            }],
        )
        .expect("seed ingestion should succeed");

        let rows = vec![LedgerRow {
            product_id: "P001".to_string(),
            department_id: "ICU".to_string(),
            dispensed_amount: Decimal::new(3, 0),
            transaction_date: Utc.with_ymd_and_hms(2026, 4, 2, 8, 0, 0).single().unwrap(),
        }];

        let dry_rows =
            build_dry_run_rows(&repo, &config, &rows).expect("dry run rows should build");
        assert_eq!(dry_rows.len(), 1);
        let row = &dry_rows[0];
        assert_eq!(row.department_id, "ICU");
        assert_eq!(
            row.opening_leftover,
            Decimal::ZERO,
            "ER carry-over must not leak into ICU"
        );
        assert_eq!(row.whole_units_output, Decimal::ZERO);
        assert_eq!(row.closing_leftover, Decimal::new(3, 0));
    }

    #[test]
    fn build_dry_run_rows_factor_one_forces_zero_leftovers() {
        let temp = tempdir().expect("tempdir should be created");
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
                factor: Decimal::ONE,
                track_subunits: false,
            }],
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let rows = vec![LedgerRow {
            product_id: "P001".to_string(),
            department_id: "ER".to_string(),
            dispensed_amount: Decimal::new(4, 0),
            transaction_date: Utc.with_ymd_and_hms(2026, 4, 2, 8, 0, 0).single().unwrap(),
        }];

        let dry_rows =
            build_dry_run_rows(&repo, &config, &rows).expect("dry run rows should build");
        assert_eq!(dry_rows.len(), 1);
        let row = &dry_rows[0];
        assert_eq!(row.opening_leftover, Decimal::ZERO);
        assert_eq!(row.closing_leftover, Decimal::ZERO);
        assert_eq!(row.total_subunits_used, Decimal::new(4, 0));
        assert_eq!(row.whole_units_output, Decimal::new(4, 0));
    }

    #[test]
    fn build_dry_run_rows_issued_includes_opening_leftover() {
        let temp = tempdir().expect("tempdir should be created");
        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        repo.commit_ingestion_batch(
            &FileHistory {
                file_hash: "prior_hash".to_string(),
                filename: "prior.xlsx".to_string(),
                file_size: 10,
                transaction_date: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).single().unwrap(),
                period_end: Utc.with_ymd_and_hms(2026, 4, 1, 8, 0, 0).single().unwrap(),
            },
            &[LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ER".to_string(),
                dispensed_amount: Decimal::new(1, 0),
                transaction_date: Utc.with_ymd_and_hms(2026, 4, 1, 8, 0, 0).single().unwrap(),
                file_hash: "prior_hash".to_string(),
                borrowed_amount: Decimal::ZERO,
            }],
        )
        .expect("seed should commit");

        repo.upsert_borrowed_carryover_batch(&[(
            "P001".to_string(),
            "ER".to_string(),
            Decimal::new(1, 0),
        )])
        .expect("carryover seed should succeed");

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

        let rows = vec![LedgerRow {
            product_id: "P001".to_string(),
            department_id: "ER".to_string(),
            dispensed_amount: Decimal::new(4, 0),
            transaction_date: Utc.with_ymd_and_hms(2026, 4, 2, 8, 0, 0).single().unwrap(),
        }];

        let dry_rows =
            build_dry_run_rows(&repo, &config, &rows).expect("dry run rows should build");
        assert_eq!(dry_rows.len(), 1);
        assert_eq!(dry_rows[0].opening_leftover, Decimal::new(1, 0));
        assert_eq!(dry_rows[0].borrowed, Decimal::new(1, 0));
        assert_eq!(dry_rows[0].issued, Decimal::new(2, 0));
    }

    #[test]
    fn commit_persists_negative_issued_carryover() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        repo.upsert_borrowed_carryover_batch(&[(
            "P001".to_string(),
            "ER".to_string(),
            Decimal::new(7, 0),
        )])
        .expect("carryover seed should succeed");

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

        let source_file_path = input_dir.join("negative_issue.xlsx");
        fs::write(&source_file_path, "placeholder").expect("source file should exist");
        let tx_date = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).single().unwrap();

        let pending = PendingIngestionCommit {
            source_file_path,
            file_hash: "hash_negative".to_string(),
            filename: "negative_issue.xlsx".to_string(),
            file_size: 11,
            transaction_date: tx_date,
            ledger_entries: vec![LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ER".to_string(),
                dispensed_amount: Decimal::new(3, 0),
                transaction_date: tx_date,
                file_hash: "hash_negative".to_string(),
                borrowed_amount: Decimal::ZERO,
            }],
            dry_run_rows: vec![],
            period_start: tx_date,
            period_end: tx_date,
            product_metadata: BTreeMap::from([(
                "P001".to_string(),
                report::ReportProductMetadata {
                    display_name: "Product 001".to_string(),
                    subunit: "Piece".to_string(),
                    unit: "Box".to_string(),
                },
            )]),
            department_metadata: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
            transaction_date_fallback_used: false,
            transaction_date_warning: None,
        };

        let report_rows = report::build_report_rows_for_entries(
            &repo,
            &config,
            &pending.ledger_entries,
            pending.period_start,
        )
        .expect("report rows should build");
        assert_eq!(report_rows.len(), 1);
        assert_eq!(report_rows[0].issued, Decimal::new(-2, 0));

        let outcome =
            commit_prepared_ingestion(&pending, &config, &repo, &reports_dir, &archive_dir)
                .expect("commit should succeed");
        assert!(outcome.report_path.exists());

        let updated = repo
            .get_borrowed_carryover("P001", "ER")
            .expect("carryover query should succeed");
        assert_eq!(updated, Decimal::new(4, 0));
    }

    #[test]
    fn commit_clears_carryover_when_consumed() {
        let temp = tempdir().expect("tempdir should be created");
        let input_dir = temp.path().join("input");
        let archive_dir = temp.path().join("archive");
        let reports_dir = temp.path().join("reports");
        fs::create_dir_all(&input_dir).expect("input dir should be created");

        let db_path = temp.path().join("state.db");
        let db = Database::new(&db_path).expect("db should initialize");
        let repo = Repository::new(&db);

        repo.upsert_borrowed_carryover_batch(&[(
            "P001".to_string(),
            "ER".to_string(),
            Decimal::new(4, 0),
        )])
        .expect("carryover seed should succeed");

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

        let source_file_path = input_dir.join("consume_carryover.xlsx");
        fs::write(&source_file_path, "placeholder").expect("source file should exist");
        let tx_date = Utc.with_ymd_and_hms(2026, 4, 4, 8, 0, 0).single().unwrap();

        let pending = PendingIngestionCommit {
            source_file_path,
            file_hash: "hash_consumed".to_string(),
            filename: "consume_carryover.xlsx".to_string(),
            file_size: 11,
            transaction_date: tx_date,
            ledger_entries: vec![LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ER".to_string(),
                dispensed_amount: Decimal::new(10, 0),
                transaction_date: tx_date,
                file_hash: "hash_consumed".to_string(),
                borrowed_amount: Decimal::ZERO,
            }],
            dry_run_rows: vec![],
            period_start: tx_date,
            period_end: tx_date,
            product_metadata: BTreeMap::from([(
                "P001".to_string(),
                report::ReportProductMetadata {
                    display_name: "Product 001".to_string(),
                    subunit: "Piece".to_string(),
                    unit: "Box".to_string(),
                },
            )]),
            department_metadata: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
            transaction_date_fallback_used: false,
            transaction_date_warning: None,
        };

        let report_rows = report::build_report_rows_for_entries(
            &repo,
            &config,
            &pending.ledger_entries,
            pending.period_start,
        )
        .expect("report rows should build");
        assert_eq!(report_rows.len(), 1);
        assert_eq!(report_rows[0].issued, Decimal::new(3, 0));

        commit_prepared_ingestion(&pending, &config, &repo, &reports_dir, &archive_dir)
            .expect("commit should succeed");

        let updated = repo
            .get_borrowed_carryover("P001", "ER")
            .expect("carryover query should succeed");
        assert_eq!(updated, Decimal::ZERO);
    }

    #[test]
    fn carryover_updates_include_opening_leftover() {
        let rows = vec![DryRunRow {
            product_id: "P001".to_string(),
            product_display_name: "Product 001".to_string(),
            department_id: "ER".to_string(),
            department_display_name: "Emergency".to_string(),
            opening_leftover: Decimal::new(1, 0),
            borrowed: Decimal::new(1, 0),
            total_subunits_used: Decimal::ZERO,
            issued: Decimal::ZERO,
            whole_units_output: Decimal::ZERO,
            closing_leftover: Decimal::ZERO,
        }];

        let updates = build_borrowed_carryover_updates(&rows);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, "P001");
        assert_eq!(updates[0].1, "ER");
        assert_eq!(updates[0].2, Decimal::ZERO);
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
            repo.get_total_for_product_department("P001", "ER")
                .expect("total query should work"),
            Decimal::new(3, 0)
        );
        assert_eq!(
            repo.get_total_for_product_department("P001", "ICU")
                .expect("total query should work"),
            Decimal::new(2, 0)
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
            repo.get_total_for_product_department("P001", "ER")
                .expect("total query should work"),
            Decimal::new(7, 0)
        );
        assert_eq!(
            repo.get_total_for_product_department("P001", "ICU")
                .expect("total query should work"),
            Decimal::new(2, 0)
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
    fn chronology_rejects_overlapping_file_ranges_based_on_ledger_max() {
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
                factor: Decimal::new(10, 0),
                track_subunits: true,
            }],
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let first_file = input_dir.join("first_span.xlsx");
        write_excel(
            &first_file,
            &[
                RowInput {
                    date_visit_be: "01-04-2569 08:00",
                    department: "ER",
                    code: "P001",
                    qty: 25,
                },
                RowInput {
                    date_visit_be: "10-04-2569 08:00",
                    department: "ER",
                    code: "P001",
                    qty: 5,
                },
            ],
        );
        ingest_excel_file(&first_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("first spanning file should ingest successfully");

        let overlapping_file = input_dir.join("overlap.xlsx");
        write_excel(
            &overlapping_file,
            &[RowInput {
                date_visit_be: "05-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 10,
            }],
        );

        let result = ingest_excel_file(
            &overlapping_file,
            &config,
            &repo,
            &archive_dir,
            &reports_dir,
        );
        assert!(matches!(
            result,
            Err(AppError::ChronologicalViolation { .. })
        ));
    }

    #[test]
    fn later_file_opening_leftover_matches_prior_report_closing_leftover() {
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
                factor: Decimal::new(10, 0),
                track_subunits: true,
            }],
            departments: BTreeMap::from([("ER".to_string(), "Emergency".to_string())]),
        };

        let first_file = input_dir.join("continuity_first.xlsx");
        write_excel(
            &first_file,
            &[RowInput {
                date_visit_be: "01-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 25,
            }],
        );

        let first = ingest_excel_file(&first_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("first file should ingest successfully");

        let first_entries = repo
            .get_ledger_entries_by_file_hash(&first.file_hash)
            .expect("first file entries should be queryable");
        let first_period_start = first_entries
            .iter()
            .map(|entry| entry.transaction_date)
            .min()
            .expect("first file should have at least one entry");
        let first_rows = report::build_report_rows_for_entries(
            &repo,
            &config,
            &first_entries,
            first_period_start,
        )
        .expect("first report rows should build");

        assert_eq!(first_rows.len(), 1);
        let first_closing = first_rows[0].closing_leftover;
        assert_eq!(first_closing, Decimal::new(5, 0));

        let second_file = input_dir.join("continuity_second.xlsx");
        write_excel(
            &second_file,
            &[RowInput {
                date_visit_be: "02-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 10,
            }],
        );

        let pending = prepare_ingestion_dry_run(&second_file, &config, &repo)
            .expect("dry run should succeed");
        assert_eq!(pending.dry_run_rows.len(), 1);
        assert_eq!(pending.dry_run_rows[0].department_id, "ER");
        assert_eq!(pending.dry_run_rows[0].opening_leftover, first_closing);
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
            repo.get_total_for_product_department("P001", "ER")
                .expect("total query should succeed"),
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
        let outcome =
            commit_prepared_ingestion(&pending, &config, &repo, &reports_dir, &archive_dir)
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
            repo.get_total_for_product_department("P001", "ER")
                .expect("total query should succeed"),
            Decimal::new(5, 0)
        );
    }

    #[test]
    fn review_dry_run_shows_only_product_department_pairs_present_in_new_file() {
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
            departments: BTreeMap::from([
                ("ER".to_string(), "Emergency".to_string()),
                ("ICU".to_string(), "Intensive Care".to_string()),
            ]),
        };

        repo.commit_ingestion_batch(
            &FileHistory {
                file_hash: "seed-review".to_string(),
                filename: "seed.xlsx".to_string(),
                file_size: 1,
                transaction_date: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).single().unwrap(),
                period_end: Utc.with_ymd_and_hms(2026, 4, 1, 10, 0, 0).single().unwrap(),
            },
            &[LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ICU".to_string(),
                dispensed_amount: Decimal::new(3, 0),
                transaction_date: Utc.with_ymd_and_hms(2026, 4, 1, 10, 0, 0).single().unwrap(),
                file_hash: "seed-review".to_string(),
                borrowed_amount: Decimal::ZERO,
            }],
        )
        .expect("seed ingestion should succeed");

        let file = input_dir.join("review_only_new_pairs.xlsx");
        write_excel(
            &file,
            &[RowInput {
                date_visit_be: "02-04-2569 08:00",
                department: "ER",
                code: "P001",
                qty: 2,
            }],
        );

        let pending = prepare_ingestion_dry_run(&file, &config, &repo)
            .expect("dry run preparation should succeed");
        assert_eq!(pending.dry_run_rows.len(), 1);
        assert_eq!(pending.dry_run_rows[0].department_id, "ER");
    }

    #[test]
    fn commit_aggregates_totals_once_per_product_department_across_multiple_files() {
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

        let first_file = input_dir.join("multi_dept_first.xlsx");
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
        ingest_excel_file(&first_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("first ingest should succeed");

        let second_file = input_dir.join("multi_dept_second.xlsx");
        write_excel(
            &second_file,
            &[
                RowInput {
                    date_visit_be: "02-04-2569 08:00",
                    department: "ER",
                    code: "P001",
                    qty: 4,
                },
                RowInput {
                    date_visit_be: "02-04-2569 09:00",
                    department: "ICU",
                    code: "P001",
                    qty: 1,
                },
            ],
        );
        ingest_excel_file(&second_file, &config, &repo, &archive_dir, &reports_dir)
            .expect("second ingest should succeed");

        assert_eq!(
            repo.get_total_for_product_department("P001", "ER")
                .expect("total query should succeed"),
            Decimal::new(7, 0)
        );
        assert_eq!(
            repo.get_total_for_product_department("P001", "ICU")
                .expect("total query should succeed"),
            Decimal::new(3, 0)
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

        let outcome =
            commit_prepared_ingestion(&pending, &config, &repo, &reports_dir, &archive_dir)
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
            period_end: seed_date
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
                borrowed_amount: Decimal::ZERO,
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
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
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
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
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
    fn parse_filename_date_range_extracts_single_month_range() {
        let (start, end) =
            parse_filename_date_range("รายงานการใช้เวชภัณฑ์ (01 Mar. 2026 - 31 Mar. 2026).xlsx")
                .expect("should parse");
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn parse_filename_date_range_extracts_cross_month_range() {
        let (start, end) =
            parse_filename_date_range("รายงานการใช้เวชภัณฑ์ (30 Mar. 2026 - 05 Apr. 2026).xlsx")
                .expect("should parse");
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 3, 30).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 4, 5).unwrap());
    }

    #[test]
    fn parse_filename_date_range_handles_thai_prefix() {
        let result =
            parse_filename_date_range("รายงานการใช้เวชภัณฑ์ (01 Mar. 2026 - 05 Apr. 2026).xlsx");
        assert!(result.is_some());
    }

    #[test]
    fn parse_filename_date_range_returns_none_for_no_parens() {
        let result = parse_filename_date_range("report-no-dates.xlsx");
        assert!(result.is_none());
    }

    #[test]
    fn parse_filename_date_range_returns_none_for_malformed_date() {
        let result = parse_filename_date_range("(30 Foo. 2026 - 05 Bar. 2026).xlsx");
        assert!(result.is_none());
    }

    #[test]
    fn parse_filename_date_range_handles_full_path() {
        let (start, end) = parse_filename_date_range(
            "/some/path/รายงานการใช้เวชภัณฑ์ (01 Mar. 2026 - 05 Apr. 2026).xlsx",
        )
        .expect("should parse");
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 4, 5).unwrap());
    }

    #[test]
    fn parse_filename_date_range_handles_single_digit_day() {
        let (start, end) =
            parse_filename_date_range("(1 Mar. 2026 - 5 Apr. 2026).xlsx").expect("should parse");
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 4, 5).unwrap());
    }

    fn make_row(date: NaiveDate, time_h: u32, time_m: u32) -> LedgerRow {
        let naive = date.and_hms_opt(time_h, time_m, 0).unwrap();
        LedgerRow {
            product_id: "P001".to_string(),
            department_id: "ER".to_string(),
            dispensed_amount: Decimal::ONE,
            transaction_date: DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc),
        }
    }

    #[test]
    fn repair_swaps_corrupted_excel_native_date_into_range() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap(),
            14,
            30,
        )];
        let sources = vec![DateSource::ExcelNative];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 1);
        assert_eq!(
            rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 3).unwrap()
        );
        assert_eq!(rows[0].transaction_date.hour(), 14);
        assert_eq!(rows[0].transaction_date.minute(), 30);
    }

    #[test]
    fn repair_preserves_string_source_dates() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap(),
            14,
            30,
        )];
        let sources = vec![DateSource::StringSource];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
        assert_eq!(
            rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
        );
    }

    #[test]
    fn repair_skips_dates_already_within_range() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            10,
            0,
        )];
        let sources = vec![DateSource::ExcelNative];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
        assert_eq!(
            rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
        );
    }

    #[test]
    fn repair_skips_when_swap_still_outside_range() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 6, 2).unwrap(),
            10,
            0,
        )];
        let sources = vec![DateSource::ExcelNative];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
        assert_eq!(
            rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 6, 2).unwrap()
        );
    }

    #[test]
    fn repair_skips_when_day_exceeds_12() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 5, 15).unwrap(),
            10,
            0,
        )];
        let sources = vec![DateSource::ExcelNative];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
        assert_eq!(
            rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()
        );
    }

    #[test]
    fn repair_handles_mixed_sources_and_dates() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![
            make_row(NaiveDate::from_ymd_opt(2026, 3, 4).unwrap(), 8, 0),
            make_row(NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(), 9, 0),
            make_row(NaiveDate::from_ymd_opt(2026, 3, 4).unwrap(), 10, 0),
        ];
        let sources = vec![
            DateSource::ExcelNative,
            DateSource::ExcelNative,
            DateSource::StringSource,
        ];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 1);
        assert_eq!(
            rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 3).unwrap()
        );
        assert_eq!(
            rows[1].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
        );
        assert_eq!(
            rows[2].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
        );
    }

    #[test]
    fn repair_returns_zero_for_empty_input() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows: Vec<LedgerRow> = vec![];
        let sources: Vec<DateSource> = vec![];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
    }

    #[test]
    fn repair_cross_month_corruption() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            14,
            30,
        )];
        let sources = vec![DateSource::ExcelNative];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
        assert_eq!(
            rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 3).unwrap()
        );
    }

    #[test]
    fn repair_boundary_start_date() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            8,
            0,
        )];
        let sources = vec![DateSource::ExcelNative];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
    }

    #[test]
    fn repair_boundary_end_date() {
        let range = (
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        );
        let mut rows = vec![make_row(
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
            23,
            59,
        )];
        let sources = vec![DateSource::ExcelNative];
        let count = repair_excel_corrupted_dates(&mut rows, &sources, range);
        assert_eq!(count, 0);
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
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
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
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
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
            period_end: Utc
                .with_ymd_and_hms(2100, 1, 1, 0, 0, 0)
                .single()
                .expect("valid date"),
        };
        repo.commit_ingestion_batch(
            &existing_history,
            &[LedgerEntry {
                product_id: "P001".to_string(),
                department_id: "ER".to_string(),
                dispensed_amount: Decimal::ONE,
                transaction_date: Utc
                    .with_ymd_and_hms(2100, 1, 1, 0, 0, 0)
                    .single()
                    .expect("valid date"),
                file_hash: "existing".to_string(),
                borrowed_amount: Decimal::ZERO,
            }],
        )
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
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
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
            .write_string(0, DATE_VISIT_COL_IDX as u16, "  order date ")
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

    #[test]
    fn extract_transaction_date_handles_all_cell_types() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("date_parsing.xlsx");
        let file_mtime = Utc::now();

        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("ParseDate").unwrap();

        // 1. Text string in BE format: "01-04-2569 08:00" -> expected 2026-04-01 08:00 UTC
        sheet.write_string(0, 0, "01-04-2569 08:00").unwrap();

        // 2. Native Excel datetime written using rust_xlsxwriter's ExcelDateTime
        let dt_native =
            rust_xlsxwriter::ExcelDateTime::parse_from_str("2026-04-01T08:00:00").unwrap();
        sheet.write_datetime(1, 0, &dt_native).unwrap();

        // 3. Excel serial number as float (approximation of 2026-04-01 08:00)
        // 46113.333333333
        let serial_val: f64 = 46113.333333333336;
        sheet.write_number(2, 0, serial_val).unwrap();

        // 4. ISO string
        sheet.write_string(3, 0, "2026-04-01T08:00:00").unwrap();

        // 5. Empty cell
        // Nothing written at row 4

        // 6. Invalid string
        sheet.write_string(5, 0, "not-a-date").unwrap();

        workbook
            .save(&workbook_path)
            .expect("workbook should be saved");

        // Parse with calamine
        let mut reader = calamine::open_workbook_auto(&workbook_path).expect("open fails");
        let sheet_names = reader.sheet_names().to_vec();
        let range = reader
            .worksheet_range(&sheet_names[0])
            .expect("range fails");

        let expected_naive =
            NaiveDateTime::parse_from_str("2026-04-01T08:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let expected_utc = DateTime::<Utc>::from_naive_utc_and_offset(expected_naive, Utc);

        // Verify 1: BE text string
        let cell0 = range.get((0, 0)).unwrap_or(&Data::Empty);
        let res0 = extract_transaction_date(cell0, file_mtime);
        assert!(!res0.is_fallback());
        assert_eq!(res0.date(), expected_utc);

        // Verify 2: Native Excel DateTime
        let cell1 = range.get((1, 0)).unwrap_or(&Data::Empty);
        let res1 = extract_transaction_date(cell1, file_mtime);
        assert!(!res1.is_fallback());
        assert_eq!(res1.date(), expected_utc);

        // Verify 3: Float serial
        let cell2 = range.get((2, 0)).unwrap_or(&Data::Empty);
        let res2 = extract_transaction_date(cell2, file_mtime);
        assert!(!res2.is_fallback());
        assert_eq!(res2.date().timestamp(), expected_utc.timestamp());

        // Verify 4: ISO string
        let cell3 = range.get((3, 0)).unwrap_or(&Data::Empty);
        let res3 = extract_transaction_date(cell3, file_mtime);
        assert!(!res3.is_fallback());
        assert_eq!(res3.date(), expected_utc);

        // Verify 5: Empty cell (Fallback to file_mtime)
        let cell4 = range.get((4, 0)).unwrap_or(&Data::Empty);
        let res4 = extract_transaction_date(cell4, file_mtime);
        assert!(res4.is_fallback());
        assert!(matches!(res4, DateExtractionResult::FallbackMissing(_)));
        assert_eq!(res4.date(), file_mtime);

        // Verify 6: Invalid string (Fallback to file_mtime)
        let cell5 = range.get((5, 0)).unwrap_or(&Data::Empty);
        let res5 = extract_transaction_date(cell5, file_mtime);
        assert!(res5.is_fallback());
        assert!(matches!(res5, DateExtractionResult::FallbackUnparseable(_)));
        assert_eq!(res5.date(), file_mtime);
    }

    #[test]
    fn extract_transaction_date_applies_be_heuristic_on_native_dates() {
        let file_mtime = Utc::now();

        // Simulate a cell with an ISO date string that has a Buddhist Era year (>2400)
        let iso_str_be = "2569-04-01T08:00:00";
        let cell = Data::DateTimeIso(iso_str_be.to_string());

        // When extracted, the heuristic should intercept it and subtract 543 from the year
        let res = extract_transaction_date(&cell, file_mtime);

        let expected_naive =
            NaiveDateTime::parse_from_str("2026-04-01T08:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let expected_utc = DateTime::<Utc>::from_naive_utc_and_offset(expected_naive, Utc);

        assert!(!res.is_fallback());
        assert_eq!(res.date(), expected_utc);
    }

    #[test]
    fn parse_excel_file_repairs_corrupted_excel_native_dates_using_filename_anchor() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("(30 Mar. 2026 - 05 Apr. 2026).xlsx");

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
        sheet.set_name("P001").expect("sheet name");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
            .unwrap();
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Consume Department")
            .unwrap();
        sheet.write_string(0, CODE_COL_IDX as u16, "Code").unwrap();
        sheet.write_string(0, QTY_COL_IDX as u16, "Qty").unwrap();

        let corrupted_dt = rust_xlsxwriter::ExcelDateTime::parse_from_str("2026-03-04T14:30:00")
            .expect("corrupted date");
        sheet
            .write_datetime(1, DATE_VISIT_COL_IDX as u16, &corrupted_dt)
            .unwrap();
        sheet
            .write_string(1, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .unwrap();
        sheet.write_string(1, CODE_COL_IDX as u16, "P001").unwrap();
        sheet.write_number(1, QTY_COL_IDX as u16, 5.0).unwrap();

        let uncorrupted_dt = rust_xlsxwriter::ExcelDateTime::parse_from_str("2026-04-01T09:00:00")
            .expect("uncorrupted date");
        sheet
            .write_datetime(2, DATE_VISIT_COL_IDX as u16, &uncorrupted_dt)
            .unwrap();
        sheet
            .write_string(2, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .unwrap();
        sheet.write_string(2, CODE_COL_IDX as u16, "P001").unwrap();
        sheet.write_number(2, QTY_COL_IDX as u16, 3.0).unwrap();

        workbook.save(&workbook_path).expect("workbook saved");

        let parsed = parse_excel_file(&workbook_path, &config).expect("parse should succeed");
        assert_eq!(parsed.rows.len(), 2);

        assert_eq!(
            parsed.rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 3).unwrap()
        );
        assert_eq!(parsed.rows[0].transaction_date.hour(), 14);
        assert_eq!(parsed.rows[0].transaction_date.minute(), 30);
        assert_eq!(parsed.rows[0].dispensed_amount, Decimal::new(5, 0));

        assert_eq!(
            parsed.rows[1].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
        );
        assert_eq!(parsed.rows[1].dispensed_amount, Decimal::new(3, 0));

        assert_eq!(
            parsed.earliest_transaction_utc.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
        );
    }

    #[test]
    fn parse_excel_file_does_not_repair_when_filename_has_no_range() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("report_no_dates.xlsx");

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
        sheet.set_name("P001").expect("sheet name");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
            .unwrap();
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Consume Department")
            .unwrap();
        sheet.write_string(0, CODE_COL_IDX as u16, "Code").unwrap();
        sheet.write_string(0, QTY_COL_IDX as u16, "Qty").unwrap();

        let corrupted_dt = rust_xlsxwriter::ExcelDateTime::parse_from_str("2026-03-04T14:30:00")
            .expect("corrupted date");
        sheet
            .write_datetime(1, DATE_VISIT_COL_IDX as u16, &corrupted_dt)
            .unwrap();
        sheet
            .write_string(1, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .unwrap();
        sheet.write_string(1, CODE_COL_IDX as u16, "P001").unwrap();
        sheet.write_number(1, QTY_COL_IDX as u16, 5.0).unwrap();

        workbook.save(&workbook_path).expect("workbook saved");

        let parsed = parse_excel_file(&workbook_path, &config).expect("parse should succeed");
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(
            parsed.rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
        );
    }

    #[test]
    fn parse_excel_file_mixed_string_and_corrupted_native_dates_repair() {
        let temp = tempdir().expect("tempdir should be created");
        let workbook_path = temp.path().join("(30 Mar. 2026 - 05 Apr. 2026).xlsx");

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
        sheet.set_name("P001").expect("sheet name");
        sheet
            .write_string(0, DATE_VISIT_COL_IDX as u16, "Order Date")
            .unwrap();
        sheet
            .write_string(0, CONSUME_DEPARTMENT_COL_IDX as u16, "Consume Department")
            .unwrap();
        sheet.write_string(0, CODE_COL_IDX as u16, "Code").unwrap();
        sheet.write_string(0, QTY_COL_IDX as u16, "Qty").unwrap();

        let corrupted_dt = rust_xlsxwriter::ExcelDateTime::parse_from_str("2026-03-04T08:00:00")
            .expect("corrupted date");
        sheet
            .write_datetime(1, DATE_VISIT_COL_IDX as u16, &corrupted_dt)
            .unwrap();
        sheet
            .write_string(1, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .unwrap();
        sheet.write_string(1, CODE_COL_IDX as u16, "P001").unwrap();
        sheet.write_number(1, QTY_COL_IDX as u16, 5.0).unwrap();

        sheet
            .write_string(2, DATE_VISIT_COL_IDX as u16, "02-04-2569 10:00")
            .unwrap();
        sheet
            .write_string(2, CONSUME_DEPARTMENT_COL_IDX as u16, "ER")
            .unwrap();
        sheet.write_string(2, CODE_COL_IDX as u16, "P001").unwrap();
        sheet.write_number(2, QTY_COL_IDX as u16, 3.0).unwrap();

        workbook.save(&workbook_path).expect("workbook saved");

        let parsed = parse_excel_file(&workbook_path, &config).expect("parse should succeed");
        assert_eq!(parsed.rows.len(), 2);

        assert_eq!(
            parsed.rows[0].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 3).unwrap()
        );

        assert_eq!(
            parsed.rows[1].transaction_date.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 2).unwrap()
        );

        assert_eq!(
            parsed.earliest_transaction_utc.date_naive(),
            NaiveDate::from_ymd_opt(2026, 4, 2).unwrap()
        );
    }
}
