use crate::db::Database;
use crate::domain::DryRunRow;
use crate::ingestion;
use crate::repository::Repository;
use crate::storage::DataDirectory;
use chrono::Local;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use super::{AppState, NethericaApp};

pub enum WorkerMessage {
    ParsingStarted {
        filename: String,
        file_size: u64,
        sheet_count: usize,
        sheet_names: Vec<String>,
    },
    ParsingLog {
        timestamp: String,
        level: String,
        message: String,
    },
    ParsingProgress {
        current_sheet: String,
        rows_processed: usize,
        total_rows: usize,
    },
    DryRunTimingComplete {
        elapsed: std::time::Duration,
    },
    Progress(String),
    DryRunData(Vec<DryRunRow>),
    DryRunPrepared(ingestion::PendingIngestionCommit),
    Completed(ingestion::IngestionOutcome),
    Error(String),
}

impl NethericaApp {
    pub(crate) fn start_ingestion_worker(&mut self, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.state = AppState::Parsing;
        self.pending_commit = None;
        self.fallback_acknowledged = true;
        self.status_message = "Starting worker...".to_string();

        // Step 3: Clear old transient state first, then set new pipeline start
        self.clear_parsing_state();
        self.clear_completion_state();
        self.pipeline_start = Some(std::time::Instant::now());

        let config = self.config.clone();

        thread::spawn(move || {
            let _ = tx.send(WorkerMessage::Progress(
                "Initializing database...".to_string(),
            ));
            let outcome = (|| {
                let db = Database::new(&config.database_path)?;
                let repository = Repository::new(&db);
                let dry_run_started = std::time::Instant::now();

                let _ = tx.send(WorkerMessage::Progress(
                    "Parsing and preparing dry-run...".to_string(),
                ));

                let prepared = ingestion::prepare_ingestion_dry_run_with_events(
                    &path,
                    &config,
                    &repository,
                    |event| match event {
                        ingestion::ParseProgressEvent::Started {
                            filename,
                            file_size,
                            sheet_count,
                            sheet_names,
                        } => {
                            let _ = tx.send(WorkerMessage::ParsingStarted {
                                filename,
                                file_size,
                                sheet_count,
                                sheet_names,
                            });
                        }
                        ingestion::ParseProgressEvent::Log { level, message } => {
                            let timestamp = Local::now().format("%H:%M:%S").to_string();
                            let _ = tx.send(WorkerMessage::ParsingLog {
                                timestamp,
                                level: level.to_string(),
                                message,
                            });
                        }
                        ingestion::ParseProgressEvent::Progress {
                            current_sheet,
                            rows_processed,
                            total_rows,
                        } => {
                            let _ = tx.send(WorkerMessage::ParsingProgress {
                                current_sheet,
                                rows_processed,
                                total_rows,
                            });
                        }
                    },
                )?;

                let _ = tx.send(WorkerMessage::DryRunTimingComplete {
                    elapsed: dry_run_started.elapsed(),
                });

                Ok::<_, crate::error::AppError>(prepared)
            })();

            match outcome {
                Ok(prepared) => {
                    let _ = tx.send(WorkerMessage::DryRunData(prepared.dry_run_rows.clone()));
                    let _ = tx.send(WorkerMessage::DryRunPrepared(prepared));
                }
                Err(err) => {
                    let _ = tx.send(WorkerMessage::Error(err.to_string()));
                }
            }
        });
    }

    pub(crate) fn start_commit_worker(&mut self, pending: ingestion::PendingIngestionCommit) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.state = AppState::Committing;
        self.status_message = "Committing transaction...".to_string();

        // Step 3 (12.8): Snapshot completion fields before moving pending into the thread
        self.completed_rows_processed = pending.ledger_entries.len();
        self.completed_filename = pending.filename.clone();
        self.completed_file_hash = pending.file_hash.clone();

        let config = self.config.clone();

        thread::spawn(move || {
            let _ = tx.send(WorkerMessage::Progress("Opening database...".to_string()));
            let outcome = (|| {
                let data_dir = DataDirectory::resolve()?;
                let db = Database::new(&config.database_path)?;
                let repository = Repository::new(&db);
                ingestion::commit_prepared_ingestion(
                    &pending,
                    &config,
                    &repository,
                    &data_dir.reports,
                    &data_dir.archive,
                )
            })();

            match outcome {
                Ok(outcome) => {
                    let _ = tx.send(WorkerMessage::Completed(outcome));
                }
                Err(err) => {
                    let _ = tx.send(WorkerMessage::Error(err.to_string()));
                }
            }
        });
    }
}
