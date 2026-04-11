use crate::error::{AppError, AppResult};
use chrono::{Duration, Local, NaiveDate};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::writer::MakeWriter;
use tracing_subscriber::{fmt, prelude::*, Registry};

const LOG_PREFIX: &str = "netherica";
const LOG_EXTENSION: &str = "log";
const MAX_FILE_SIZE_BYTES: u64 = 5 * 1024 * 1024;
const RETENTION_DAYS: i64 = 7;

/// Initializes the logging system with rolling daily logs.
///
/// # Requirements:
/// - Max 5MB per file.
/// - Keep 7 days of logs.
/// - Logs in the same directory as `state.db`.
pub fn init_logging(database_path: &Path) -> AppResult<()> {
    // Extract the directory from the database path
    let log_dir = resolve_log_dir(database_path);

    // Ensure the log directory exists
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)
            .map_err(|e| AppError::ConfigError(format!("Failed to create log directory: {}", e)))?;
    }

    // Set up policy-driven appender: daily rotation + 5MB size cap + 7-day retention.
    let file_appender = PolicyFileAppender::new(log_dir.clone())?;

    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_target(true);

    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_target(true);

    // Combine layers
    let subscriber = Registry::default().with(file_layer).with(stdout_layer);

    tracing::subscriber::set_global_default(subscriber).map_err(|e| {
        AppError::ConfigError(format!("Failed to set global tracing subscriber: {}", e))
    })?;

    tracing::info!("Logging initialized in {:?}", log_dir);

    Ok(())
}

fn resolve_log_dir(database_path: &Path) -> PathBuf {
    database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

#[derive(Debug)]
struct PolicyFileAppender {
    state: Arc<Mutex<PolicyState>>,
}

#[derive(Debug)]
struct PolicyState {
    dir: PathBuf,
    current_date: NaiveDate,
    current_index: u32,
    current_size: u64,
    file: File,
}

impl PolicyFileAppender {
    fn new(dir: PathBuf) -> AppResult<Self> {
        let today = Local::now().date_naive();
        cleanup_old_logs(&dir, today, RETENTION_DAYS)?;
        let (file, index, size) = open_latest_or_new_file(&dir, today)?;

        Ok(Self {
            state: Arc::new(Mutex::new(PolicyState {
                dir,
                current_date: today,
                current_index: index,
                current_size: size,
                file,
            })),
        })
    }
}

impl<'a> MakeWriter<'a> for PolicyFileAppender {
    type Writer = PolicyWriter;

    fn make_writer(&'a self) -> Self::Writer {
        PolicyWriter {
            state: Arc::clone(&self.state),
        }
    }
}

#[derive(Debug)]
struct PolicyWriter {
    state: Arc<Mutex<PolicyState>>,
}

impl Write for PolicyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("poisoned logging mutex"))?;

        ensure_policy(&mut state, buf.len() as u64)?;
        let written = state.file.write(buf)?;
        state.current_size = state.current_size.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("poisoned logging mutex"))?;
        state.file.flush()
    }
}

fn ensure_policy(state: &mut PolicyState, incoming_bytes: u64) -> io::Result<()> {
    let today = Local::now().date_naive();

    if today != state.current_date {
        cleanup_old_logs(&state.dir, today, RETENTION_DAYS)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let (file, index, size) = open_latest_or_new_file(&state.dir, today)
            .map_err(|e| io::Error::other(e.to_string()))?;
        state.current_date = today;
        state.current_index = index;
        state.current_size = size;
        state.file = file;
    }

    if state.current_size.saturating_add(incoming_bytes) > MAX_FILE_SIZE_BYTES {
        let next_index = state.current_index.saturating_add(1);
        let next_path = log_path(&state.dir, state.current_date, next_index);
        let file = open_append(&next_path).map_err(|e| io::Error::other(e.to_string()))?;
        let size = file.metadata()?.len();

        state.current_index = next_index;
        state.current_size = size;
        state.file = file;
    }

    Ok(())
}

fn open_latest_or_new_file(dir: &Path, date: NaiveDate) -> AppResult<(File, u32, u64)> {
    let mut highest_index = 0u32;
    let mut found = false;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some((file_date, idx)) = parse_log_file(&path) {
            if file_date == date {
                found = true;
                highest_index = highest_index.max(idx);
            }
        }
    }

    let index = if found { highest_index } else { 0 };
    let path = log_path(dir, date, index);
    let file = open_append(&path)?;
    let size = file.metadata()?.len();
    Ok((file, index, size))
}

fn open_append(path: &Path) -> AppResult<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(AppError::from)
}

fn cleanup_old_logs(dir: &Path, now: NaiveDate, retention_days: i64) -> AppResult<()> {
    let oldest_kept = now - Duration::days(retention_days - 1);

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some((file_date, _)) = parse_log_file(&path) {
            if file_date < oldest_kept {
                fs::remove_file(path)?;
            }
        }
    }

    Ok(())
}

fn log_path(dir: &Path, date: NaiveDate, index: u32) -> PathBuf {
    if index == 0 {
        dir.join(format!(
            "{LOG_PREFIX}-{}.{}",
            date.format("%Y-%m-%d"),
            LOG_EXTENSION
        ))
    } else {
        dir.join(format!(
            "{LOG_PREFIX}-{}.{}.{}",
            date.format("%Y-%m-%d"),
            index,
            LOG_EXTENSION
        ))
    }
}

fn parse_log_file(path: &Path) -> Option<(NaiveDate, u32)> {
    if path.extension() != Some(OsStr::new(LOG_EXTENSION)) {
        return None;
    }

    let stem = path.file_stem()?.to_str()?;
    let prefix = format!("{LOG_PREFIX}-");
    if !stem.starts_with(&prefix) {
        return None;
    }

    let remainder = &stem[prefix.len()..];
    let mut split = remainder.splitn(2, '.');
    let date_str = split.next()?;
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let idx = split
        .next()
        .map(|v| v.parse::<u32>().ok())
        .unwrap_or(Some(0))?;

    Some((date, idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_file_handles_base_and_indexed_names() {
        let p1 = PathBuf::from("netherica-2026-04-09.log");
        let p2 = PathBuf::from("netherica-2026-04-09.3.log");
        let p3 = PathBuf::from("other-2026-04-09.log");

        assert_eq!(
            parse_log_file(&p1),
            Some((NaiveDate::from_ymd_opt(2026, 4, 9).unwrap(), 0))
        );
        assert_eq!(
            parse_log_file(&p2),
            Some((NaiveDate::from_ymd_opt(2026, 4, 9).unwrap(), 3))
        );
        assert_eq!(parse_log_file(&p3), None);
    }

    #[test]
    fn cleanup_old_logs_keeps_only_retention_window() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();

        let keep_date = NaiveDate::from_ymd_opt(2026, 4, 9).unwrap();
        let remove_date = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();

        fs::write(log_path(dir, keep_date, 0), b"keep").unwrap();
        fs::write(log_path(dir, remove_date, 0), b"remove").unwrap();

        cleanup_old_logs(dir, keep_date, 7).unwrap();

        assert!(log_path(dir, keep_date, 0).exists());
        assert!(!log_path(dir, remove_date, 0).exists());
    }

    #[test]
    fn resolve_log_dir_defaults_to_current_dir_for_bare_filename() {
        let dir = resolve_log_dir(Path::new("state.db"));
        assert_eq!(dir, PathBuf::from("."));
    }

    #[test]
    fn resolve_log_dir_uses_parent_for_nested_database_path() {
        let dir = resolve_log_dir(Path::new("data/state.db"));
        assert_eq!(dir, PathBuf::from("data"));
    }
}
