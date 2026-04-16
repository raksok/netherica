mod components;
mod sidebar;
pub(crate) mod theme;
mod views;
mod worker;

pub use worker::WorkerMessage;

use crate::config::Config;
use crate::db::Database;
use crate::ingestion;
use crate::report;
use crate::repository::Repository;
use crate::storage::DataDirectory;
use chrono::{DateTime, Utc};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Parsing,
    ParsingHold,
    DryRun,
    Committing,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationSection {
    Ingestion,
    Reports,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Departments,
    Products,
}

#[derive(Debug, Clone)]
pub struct ParsingFileMetadata {
    pub filename: String,
    pub file_size: u64,
    pub sheet_count: usize,
    pub sheet_names: Vec<String>,
}

pub struct NethericaApp {
    pub state: AppState,
    pub config: Config,
    pub selected_file: Option<PathBuf>,
    pub status_message: String,
    pub receiver: Option<mpsc::Receiver<WorkerMessage>>,
    pub dry_run_data: Vec<crate::domain::DryRunRow>,
    pub pending_commit: Option<ingestion::PendingIngestionCommit>,
    pub toast_message: Option<(String, std::time::Instant)>,
    pub critical_error: Option<String>,
    pub fallback_acknowledged: bool,
    pub last_report_path: Option<PathBuf>,
    pub post_generation_guidance: Option<String>,
    pub storage_fallback_warning_shown: bool,
    pub active_section: NavigationSection,
    pub active_settings_tab: SettingsTab,

    // Step 3 (12.6): Startup summary state for Idle view
    pub last_run_timestamp: Option<DateTime<Utc>>,
    pub db_connected: bool,
    pub storage_source: Option<crate::storage::DataRootSource>,

    // Step 3 (12.7): Structured parsing/progress view state
    pub parsing_logs: Vec<(String, String, String)>,
    pub parsing_file_metadata: Option<ParsingFileMetadata>,
    pub parsing_progress: Option<(String, usize, usize)>,

    // Step 3 (12.8): Completion summary fields
    pub completed_rows_processed: usize,
    pub completed_filename: String,
    pub completed_file_hash: String,
    pub completed_archive_move_pending: bool,
    pub pipeline_start: Option<std::time::Instant>,
    pub dry_run_elapsed: Option<std::time::Duration>,
    pub parse_hold_until: Option<std::time::Instant>,
}

impl NethericaApp {
    fn from_config(config: Config) -> Self {
        Self {
            state: AppState::Idle,
            config,
            selected_file: None,
            status_message: "Ready".to_string(),
            receiver: None,
            dry_run_data: Vec::new(),
            pending_commit: None,
            toast_message: None,
            critical_error: None,
            fallback_acknowledged: true,
            last_report_path: None,
            post_generation_guidance: None,
            storage_fallback_warning_shown: false,
            active_section: NavigationSection::Ingestion,
            active_settings_tab: SettingsTab::Departments,

            // Step 3 (12.6)
            last_run_timestamp: None,
            db_connected: false,
            storage_source: None,

            // Step 3 (12.7)
            parsing_logs: Vec::new(),
            parsing_file_metadata: None,
            parsing_progress: None,

            // Step 3 (12.8)
            completed_rows_processed: 0,
            completed_filename: String::new(),
            completed_file_hash: String::new(),
            completed_archive_move_pending: false,
            pipeline_start: None,
            dry_run_elapsed: None,
            parse_hold_until: None,
        }
    }

    pub fn new(_cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        theme::configure_egui_fonts(&_cc.egui_ctx);
        theme::apply_design_system(&_cc.egui_ctx);

        let mut app = Self::from_config(config);

        // Step 3 (12.6): Startup probe 1 — storage source
        if let Ok(data_dir) = DataDirectory::resolve() {
            app.storage_source = Some(data_dir.root_source);
            app.maybe_show_storage_fallback_warning(&data_dir);
        }

        // Step 3 (12.6): Startup probe 2 — database + last run timestamp
        match Database::new(&app.config.database_path) {
            Ok(db) => {
                let repository = Repository::new(&db);
                app.db_connected = true;
                app.last_run_timestamp = repository.get_max_transaction_date().ok().flatten();
            }
            Err(_) => {
                app.db_connected = false;
                app.last_run_timestamp = None;
            }
        }

        app
    }

    fn maybe_show_storage_fallback_warning(&mut self, data_dir: &DataDirectory) {
        if self.storage_fallback_warning_shown || !data_dir.used_fallback() {
            return;
        }

        self.toast_message = Some((
            format!(
                "Storage fallback active: executable directory is not writable. Using OS user data directory: {}",
                data_dir.root.display()
            ),
            std::time::Instant::now(),
        ));
        self.storage_fallback_warning_shown = true;
    }

    fn can_confirm_commit(&self) -> bool {
        self.pending_commit.is_some()
            && (self.fallback_acknowledged
                || !self
                    .pending_commit
                    .as_ref()
                    .map(|p| p.transaction_date_fallback_used)
                    .unwrap_or(false))
    }

    fn clear_parsing_state(&mut self) {
        self.parsing_logs.clear();
        self.parsing_file_metadata = None;
        self.parsing_progress = None;
        self.parse_hold_until = None;
    }

    fn begin_parsing_hold(&mut self) {
        self.state = AppState::ParsingHold;
        self.parse_hold_until = Some(Instant::now() + Duration::from_millis(900));
    }

    fn can_continue_to_dry_run(&self) -> bool {
        self.parse_hold_until
            .map(|until| Instant::now() >= until)
            .unwrap_or(true)
    }

    fn finish_parsing_hold(&mut self) {
        self.state = AppState::DryRun;
        self.parse_hold_until = None;
    }

    fn clear_completion_state(&mut self) {
        self.completed_rows_processed = 0;
        self.completed_filename.clear();
        self.completed_file_hash.clear();
        self.completed_archive_move_pending = false;
        self.pipeline_start = None;
        self.dry_run_elapsed = None;
    }

    fn handle_dry_run_prepared(&mut self, prepared: ingestion::PendingIngestionCommit) {
        let fallback_used = prepared.transaction_date_fallback_used;
        let warning = prepared.transaction_date_warning.clone();
        self.pending_commit = Some(prepared);
        self.fallback_acknowledged = !fallback_used;
        self.status_message = "Dry-run ready. Review and confirm to commit.".to_string();

        if let Some(message) = warning {
            self.toast_message = Some((message, std::time::Instant::now()));
        }
    }

    fn process_worker_messages(&mut self) {
        if let Some(rx) = self.receiver.take() {
            let current_rx = rx;
            loop {
                match current_rx.try_recv() {
                    Ok(msg) => match msg {
                        WorkerMessage::ParsingStarted {
                            filename,
                            file_size,
                            sheet_count,
                            sheet_names,
                        } => {
                            self.parsing_file_metadata = Some(ParsingFileMetadata {
                                filename,
                                file_size,
                                sheet_count,
                                sheet_names,
                            });
                        }
                        WorkerMessage::ParsingLog {
                            timestamp,
                            level,
                            message,
                        } => {
                            self.parsing_logs.push((timestamp, level, message));
                        }
                        WorkerMessage::ParsingProgress {
                            current_sheet,
                            rows_processed,
                            total_rows,
                        } => {
                            self.parsing_progress =
                                Some((current_sheet, rows_processed, total_rows));
                        }
                        WorkerMessage::DryRunTimingComplete { elapsed } => {
                            self.dry_run_elapsed = Some(elapsed);
                        }
                        WorkerMessage::Progress(progress) => {
                            self.status_message = progress;
                        }
                        WorkerMessage::DryRunData(data) => {
                            self.dry_run_data = data;
                        }
                        WorkerMessage::DryRunPrepared(prepared) => {
                            self.handle_dry_run_prepared(prepared);
                            self.begin_parsing_hold();
                        }
                        WorkerMessage::Completed(outcome) => {
                            // Step 3 (12.8): Capture completion snapshot before report-ready flow
                            self.completed_archive_move_pending = outcome.archive_move_pending;

                            self.handle_report_ready(outcome.report_path.clone(), "generated");

                            if outcome.archive_move_pending {
                                self.status_message =
                                    "Commit succeeded. Report ready. Archive move queued for retry."
                                        .to_string();
                                self.toast_message = Some((
                                    "Warning: file committed but archive move failed; queued for retry."
                                        .to_string(),
                                    std::time::Instant::now(),
                                ));
                            } else {
                                self.status_message =
                                    "Process completed successfully. Report is ready.".to_string();
                            }
                            self.state = AppState::Complete;
                            break;
                        }
                        WorkerMessage::Error(err) => {
                            self.status_message = "Process failed.".to_string();
                            self.critical_error = Some(err);
                            self.state = AppState::Idle;
                            self.pending_commit = None;
                            self.fallback_acknowledged = true;
                            break;
                        }
                    },
                    Err(mpsc::TryRecvError::Empty) => {
                        self.receiver = Some(current_rx);
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.receiver = None;
                        break;
                    }
                }
            }
        }
    }

    fn regenerate_last_report(&mut self) {
        let outcome = (|| {
            let db = crate::db::Database::new(&self.config.database_path)?;
            let repository = crate::repository::Repository::new(&db);
            let data_dir = DataDirectory::resolve()?;
            report::regenerate_last_report(&repository, &self.config, &data_dir.reports)
        })();

        match outcome {
            Ok(path) => {
                self.handle_report_ready(path.clone(), "regenerated");
                self.status_message = format!("Report regenerated: {}", path.display());
            }
            Err(err) => {
                self.status_message = "Report regeneration failed.".to_string();
                self.critical_error = Some(err.to_string());
            }
        }
    }

    fn handle_report_ready(&mut self, report_path: PathBuf, origin: &str) {
        self.last_report_path = Some(report_path.clone());
        self.post_generation_guidance = Some(build_print_guidance_message(&report_path));

        match open_path_in_default_app(&report_path, OpenTarget::ReportFile) {
            Ok(()) => {
                self.toast_message = Some((
                    format!("Report {origin} and opened: {}", report_path.display()),
                    std::time::Instant::now(),
                ));
            }
            Err(err) => {
                warn!(
                    path = %report_path.display(),
                    error = %err,
                    "Failed to auto-open generated report"
                );
                self.toast_message = Some((
                    format!(
                        "Report {origin}, but auto-open failed. Open manually: {}",
                        report_path.display()
                    ),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    fn open_report_folder_action(&mut self) {
        let outcome = (|| {
            let folder = resolve_report_folder(self.last_report_path.as_deref(), || {
                DataDirectory::resolve().map(|d| d.reports)
            })?;
            open_path_in_default_app(&folder, OpenTarget::ReportFolder)?;
            Ok::<PathBuf, crate::error::AppError>(folder)
        })();

        match outcome {
            Ok(folder) => {
                self.toast_message = Some((
                    format!("Opening report folder: {}", folder.display()),
                    std::time::Instant::now(),
                ));
            }
            Err(err) => {
                warn!(error = %err, "Failed to open report folder");
                self.toast_message = Some((
                    format!("Unable to open report folder: {err}"),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    fn open_latest_report_action(&mut self) {
        let Some(path) = self.last_report_path.clone() else {
            self.toast_message = Some((
                "No recent report found to open.".to_string(),
                std::time::Instant::now(),
            ));
            return;
        };

        match open_path_in_default_app(&path, OpenTarget::ReportFile) {
            Ok(()) => {
                self.toast_message = Some((
                    format!("Opening report: {}", path.display()),
                    std::time::Instant::now(),
                ));
            }
            Err(err) => {
                warn!(path = %path.display(), error = %err, "Failed to open latest report");
                self.toast_message = Some((
                    format!("Unable to open report: {err}"),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    fn render_toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((message, shown_at)) = self.toast_message.clone() else {
            return;
        };

        if shown_at.elapsed() > std::time::Duration::from_secs(5) {
            self.toast_message = None;
            return;
        }

        let mut dismiss = false;

        egui::Area::new(egui::Id::new("toast_overlay"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0))
            .show(ctx, |ui| {
                components::overlay_card_frame(theme::SURFACE_CONTAINER_HIGHEST).show(ui, |ui| {
                    ui.set_max_width(520.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Notice")
                                .size(11.0)
                                .strong()
                                .color(theme::ON_SURFACE_VARIANT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if components::ghost_button(
                                ui,
                                egui::RichText::new("Close")
                                    .size(11.0)
                                    .color(theme::ON_SURFACE_VARIANT),
                            )
                            .clicked()
                            {
                                dismiss = true;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(message).color(theme::ON_SURFACE));
                });
            });

        if dismiss {
            self.toast_message = None;
        }
    }

    fn render_error_overlay(&mut self, ctx: &egui::Context) {
        let Some(error_message) = self.critical_error.clone() else {
            return;
        };

        let mut close = false;

        egui::Area::new(egui::Id::new("error_overlay_scrim"))
            .order(egui::Order::Foreground)
            .interactable(true)
            .movable(false)
            .anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let rect = ctx.screen_rect();
                ui.painter().rect_filled(rect, 0.0, theme::MODAL_OVERLAY);
            });

        egui::Area::new(egui::Id::new("error_overlay_modal"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                components::overlay_card_frame(theme::SURFACE_CONTAINER_HIGH).show(ui, |ui| {
                    ui.set_width(560.0);
                    ui.label(
                        egui::RichText::new("Error")
                            .size(22.0)
                            .strong()
                            .color(theme::ERROR),
                    );
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(error_message).color(theme::ON_SURFACE));
                    ui.add_space(16.0);
                    if components::primary_button(
                        ui,
                        egui::RichText::new("Close").color(theme::ON_PRIMARY_CONTAINER),
                    )
                    .clicked()
                    {
                        close = true;
                    }
                });
            });

        if close {
            self.critical_error = None;
        }
    }

    fn render_report_ready_overlay(&mut self, ctx: &egui::Context) {
        let Some(message) = self.post_generation_guidance.clone() else {
            return;
        };

        let mut close = false;

        egui::Area::new(egui::Id::new("report_ready_overlay_scrim"))
            .order(egui::Order::Foreground)
            .interactable(true)
            .movable(false)
            .anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let rect = ctx.screen_rect();
                ui.painter().rect_filled(rect, 0.0, theme::MODAL_OVERLAY);
            });

        egui::Area::new(egui::Id::new("report_ready_overlay_modal"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                components::overlay_card_frame(theme::SURFACE_CONTAINER_HIGH).show(ui, |ui| {
                    ui.set_width(620.0);
                    ui.label(
                        egui::RichText::new("Report Ready")
                            .size(22.0)
                            .strong()
                            .color(theme::PRIMARY),
                    );
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(message).color(theme::ON_SURFACE));
                    ui.add_space(16.0);
                    ui.horizontal_wrapped(|ui| {
                        if components::primary_button(
                            ui,
                            egui::RichText::new("Open Report").color(theme::ON_PRIMARY_CONTAINER),
                        )
                        .clicked()
                        {
                            self.open_latest_report_action();
                        }
                        if components::secondary_button(ui, "Open Report Folder").clicked() {
                            self.open_report_folder_action();
                        }
                        if components::ghost_button(
                            ui,
                            egui::RichText::new("Close").color(theme::ON_SURFACE_VARIANT),
                        )
                        .clicked()
                        {
                            close = true;
                        }
                    });
                });
            });

        if close {
            self.post_generation_guidance = None;
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum OpenTarget {
    ReportFile,
    ReportFolder,
}

impl OpenTarget {
    fn label(self) -> &'static str {
        match self {
            Self::ReportFile => "report",
            Self::ReportFolder => "report folder",
        }
    }
}

fn build_print_guidance_message(report_path: &Path) -> String {
    format!(
        "Report ready: {}\nUse Ctrl+P in your browser to print or save as PDF.",
        report_path.display()
    )
}

fn resolve_report_folder<F>(
    last_report_path: Option<&Path>,
    reports_dir_resolver: F,
) -> crate::error::AppResult<PathBuf>
where
    F: FnOnce() -> crate::error::AppResult<PathBuf>,
{
    if let Some(parent) = last_report_path.and_then(Path::parent) {
        return Ok(parent.to_path_buf());
    }

    reports_dir_resolver()
}

fn open_path_in_default_app(path: &Path, target: OpenTarget) -> crate::error::AppResult<()> {
    if !path.exists() {
        return Err(crate::error::AppError::DomainError(format!(
            "{} does not exist: {}",
            target.label(),
            path.display()
        )));
    }

    let mut command = match std::env::consts::OS {
        "windows" => {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg("start").arg("").arg(path.as_os_str());
            cmd
        }
        "linux" => {
            let mut cmd = Command::new("xdg-open");
            cmd.arg(path.as_os_str());
            cmd
        }
        other => {
            return Err(crate::error::AppError::InternalError(format!(
                "Unsupported OS for opening {}: {other}",
                target.label()
            )));
        }
    };

    command.spawn().map_err(|e| {
        crate::error::AppError::InternalError(format!(
            "Failed to open {} '{}': {e}",
            target.label(),
            path.display()
        ))
    })?;

    Ok(())
}

impl eframe::App for NethericaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_worker_messages();

        if self.receiver.is_some() {
            ctx.request_repaint();
        }

        if self.state == AppState::ParsingHold {
            if let Some(until) = self.parse_hold_until {
                let now = Instant::now();
                if until > now {
                    ctx.request_repaint_after(until - now);
                }
            }
        }

        self.render_sidebar(ctx);

        egui::TopBottomPanel::bottom("status_bar")
            .resizable(false)
            .exact_height(theme::STATUS_BAR_HEIGHT)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Precision Reconciliation Engine")
                            .size(11.0)
                            .color(theme::ON_SURFACE_VARIANT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:?} — {}",
                                self.state, self.status_message
                            ))
                            .size(11.0)
                            .color(theme::ON_SURFACE),
                        );
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_toast_overlay(ctx);
            self.render_error_overlay(ctx);
            self.render_report_ready_overlay(ctx);

            match self.active_section {
                NavigationSection::Settings => self.render_scrolling_main_canvas(ui),
                NavigationSection::Ingestion | NavigationSection::Reports => {
                    self.render_main_canvas(ui)
                }
            }
        });
    }
}

impl NethericaApp {
    fn render_main_canvas(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            egui::vec2(theme::MAIN_CANVAS_WIDTH, theme::MAIN_CANVAS_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(egui::vec2(
                    theme::MAIN_CANVAS_WIDTH,
                    theme::MAIN_CANVAS_HEIGHT,
                ));
                ui.set_max_size(egui::vec2(
                    theme::MAIN_CANVAS_WIDTH,
                    theme::MAIN_CANVAS_HEIGHT,
                ));

                egui::Frame::none()
                    .inner_margin(egui::Margin::same(theme::CANVAS_PADDING))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(theme::CONTENT_WIDTH, theme::CONTENT_HEIGHT));
                        ui.set_max_size(egui::vec2(theme::CONTENT_WIDTH, theme::CONTENT_HEIGHT));

                        match self.active_section {
                            NavigationSection::Ingestion => self.render_ingestion_section(ui),
                            NavigationSection::Reports => self.render_reports_view(ui),
                            NavigationSection::Settings => self.render_settings_view(ui),
                        }
                    });
            },
        );
    }

    fn render_scrolling_main_canvas(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            egui::vec2(theme::MAIN_CANVAS_WIDTH, theme::MAIN_CANVAS_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(egui::vec2(
                    theme::MAIN_CANVAS_WIDTH,
                    theme::MAIN_CANVAS_HEIGHT,
                ));
                ui.set_max_size(egui::vec2(
                    theme::MAIN_CANVAS_WIDTH,
                    theme::MAIN_CANVAS_HEIGHT,
                ));

                egui::Frame::none()
                    .inner_margin(egui::Margin::same(theme::CANVAS_PADDING))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_source("main_settings_canvas_scroll")
                            .show(ui, |ui| {
                                ui.set_width(theme::CONTENT_WIDTH);
                                self.render_settings_view(ui);
                            });
                    });
            },
        );
    }

    fn render_ingestion_section(&mut self, ui: &mut egui::Ui) {
        match self.state {
            AppState::Idle => self.render_idle_view(ui),
            AppState::Parsing | AppState::ParsingHold => self.render_parsing_view(ui),
            AppState::DryRun => self.render_dry_run_view(ui),
            AppState::Committing => self.render_committing_view(ui),
            AppState::Complete => self.render_complete_view(ui),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColumnNames, Settings};
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;
    use std::sync::mpsc;

    fn test_config() -> Config {
        Config {
            database_path: PathBuf::from("state.db"),
            settings: Settings {
                strict_chronological: true,
            },
            column_names: ColumnNames::default(),
            products: vec![],
            departments: BTreeMap::new(),
        }
    }

    fn test_pending(fallback_used: bool) -> ingestion::PendingIngestionCommit {
        let dt = Utc
            .with_ymd_and_hms(2026, 4, 1, 8, 0, 0)
            .single()
            .expect("valid datetime");
        ingestion::PendingIngestionCommit {
            source_file_path: PathBuf::from("input.xlsx"),
            file_hash: "hash".to_string(),
            filename: "input.xlsx".to_string(),
            file_size: 42,
            transaction_date: dt,
            ledger_entries: vec![],
            dry_run_rows: vec![],
            period_start: dt,
            period_end: dt,
            product_metadata: BTreeMap::new(),
            department_metadata: BTreeMap::new(),
            transaction_date_fallback_used: fallback_used,
            transaction_date_warning: fallback_used
                .then_some("Fallback warning: using file modification timestamp".to_string()),
        }
    }

    #[test]
    fn fallback_prepared_payload_requires_ack_before_confirm() {
        let mut app = NethericaApp::from_config(test_config());
        app.handle_dry_run_prepared(test_pending(true));

        assert!(!app.fallback_acknowledged);
        assert!(!app.can_confirm_commit());
        assert!(app.toast_message.is_some());

        app.fallback_acknowledged = true;
        assert!(app.can_confirm_commit());
    }

    #[test]
    fn non_fallback_prepared_payload_can_confirm_immediately() {
        let mut app = NethericaApp::from_config(test_config());
        app.handle_dry_run_prepared(test_pending(false));

        assert!(app.fallback_acknowledged);
        assert!(app.can_confirm_commit());
    }

    #[test]
    fn print_guidance_contains_path_and_ctrl_p_instruction() {
        let message =
            build_print_guidance_message(Path::new("reports/20260409_101010_report.html"));

        assert!(message.contains("Report ready:"));
        assert!(message.contains("Ctrl+P"));
        assert!(message.contains("report.html"));
    }

    #[test]
    fn resolve_report_folder_prefers_latest_report_parent() {
        let folder = resolve_report_folder(
            Some(Path::new("D:/tmp/reports/20260409_101010_report.html")),
            || Ok(PathBuf::from("fallback/reports")),
        )
        .expect("should resolve parent folder from latest report");

        assert!(folder.ends_with(Path::new("tmp/reports")));
    }

    #[test]
    fn resolve_report_folder_falls_back_to_configured_reports_dir() {
        let folder = resolve_report_folder(None, || Ok(PathBuf::from("fallback/reports")))
            .expect("should fall back when no report path is known");

        assert_eq!(folder, PathBuf::from("fallback/reports"));
    }

    #[test]
    fn storage_fallback_warning_is_shown_only_once() {
        let mut app = NethericaApp::from_config(test_config());
        let fallback_data_dir = DataDirectory {
            root: PathBuf::from("C:/Users/test/AppData/Roaming/netherica"),
            archive: PathBuf::from("C:/Users/test/AppData/Roaming/netherica/archive"),
            reports: PathBuf::from("C:/Users/test/AppData/Roaming/netherica/reports"),
            root_source: crate::storage::DataRootSource::OsUserDataFallback,
        };

        app.maybe_show_storage_fallback_warning(&fallback_data_dir);
        let first_toast = app.toast_message.clone();

        app.maybe_show_storage_fallback_warning(&fallback_data_dir);

        assert!(app.storage_fallback_warning_shown);
        assert_eq!(app.toast_message, first_toast);
    }

    #[test]
    fn storage_executable_root_does_not_show_fallback_warning() {
        let mut app = NethericaApp::from_config(test_config());
        let executable_data_dir = DataDirectory {
            root: PathBuf::from("D:/apps/netherica"),
            archive: PathBuf::from("D:/apps/netherica/archive"),
            reports: PathBuf::from("D:/apps/netherica/reports"),
            root_source: crate::storage::DataRootSource::ExecutableDirectory,
        };

        app.maybe_show_storage_fallback_warning(&executable_data_dir);

        assert!(!app.storage_fallback_warning_shown);
        assert!(app.toast_message.is_none());
    }

    #[test]
    fn process_worker_messages_updates_structured_parsing_state() {
        let mut app = NethericaApp::from_config(test_config());
        let (tx, rx) = mpsc::channel();
        app.receiver = Some(rx);

        tx.send(WorkerMessage::ParsingStarted {
            filename: "input.xlsx".to_string(),
            file_size: 128,
            sheet_count: 2,
            sheet_names: vec!["GAUZE-01".to_string(), "SYR-MED-04".to_string()],
        })
        .expect("send parsing start");
        tx.send(WorkerMessage::ParsingLog {
            timestamp: "10:00:01".to_string(),
            level: "INFO".to_string(),
            message: "Opening sheet 'GAUZE-01'".to_string(),
        })
        .expect("send parsing log");
        tx.send(WorkerMessage::ParsingProgress {
            current_sheet: "GAUZE-01".to_string(),
            rows_processed: 25,
            total_rows: 100,
        })
        .expect("send parsing progress");
        tx.send(WorkerMessage::DryRunTimingComplete {
            elapsed: std::time::Duration::from_secs(2),
        })
        .expect("send dry-run timing");
        drop(tx);

        app.process_worker_messages();

        let metadata = app.parsing_file_metadata.expect("metadata should be set");
        assert_eq!(metadata.filename, "input.xlsx");
        assert_eq!(metadata.file_size, 128);
        assert_eq!(metadata.sheet_count, 2);
        assert_eq!(metadata.sheet_names, vec!["GAUZE-01", "SYR-MED-04"]);

        assert_eq!(app.parsing_logs.len(), 1);
        assert_eq!(
            app.parsing_logs[0],
            (
                "10:00:01".to_string(),
                "INFO".to_string(),
                "Opening sheet 'GAUZE-01'".to_string()
            )
        );
        assert_eq!(
            app.parsing_progress,
            Some(("GAUZE-01".to_string(), 25, 100))
        );
        assert_eq!(app.dry_run_elapsed, Some(std::time::Duration::from_secs(2)));
    }
}
