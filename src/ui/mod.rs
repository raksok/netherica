use crate::config::Config;
use crate::db::Database;
use crate::domain::DryRunRow;
use crate::ingestion;
use crate::report;
use crate::repository::Repository;
use crate::storage::DataDirectory;
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use tracing::warn;

const UI_THAI_FONT_NAME: &str = "noto_sans_thai_looped_regular";
const UI_THAI_FONT_BYTES: &[u8] = include_bytes!("../../asset/NotoSansThaiLooped-Regular.ttf");

fn build_font_definitions_with_utf8_support() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        UI_THAI_FONT_NAME.to_string(),
        egui::FontData::from_static(UI_THAI_FONT_BYTES),
    );

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, UI_THAI_FONT_NAME.to_string());
    }

    fonts
}

fn configure_egui_fonts(ctx: &egui::Context) {
    ctx.set_fonts(build_font_definitions_with_utf8_support());
}

fn apply_design_system(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();

    // 1. Spacing & Layout
    // Spacing scale 2 (base 4px). item_spacing = space-3 (12px), window_margin = space-6 (24px)
    style.spacing.item_spacing = egui::vec2(12.0, 12.0);
    style.spacing.window_margin = egui::Margin::same(24.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);

    // 2. Color Tokens (Nordic Precision - The Arctic Atelier)
    let primary = egui::Color32::from_rgb(163, 220, 236); // #a3dcec
    let surface_container_lowest = egui::Color32::from_rgb(8, 14, 25); // #080e19
    let surface = egui::Color32::from_rgb(13, 19, 30); // #0d131e
    let surface_container_low = egui::Color32::from_rgb(22, 28, 39); // #161c27
    let surface_container = egui::Color32::from_rgb(26, 32, 43); // #1a202b
    let surface_container_high = egui::Color32::from_rgb(36, 42, 54); // #242a36

    let on_surface = egui::Color32::from_rgb(221, 226, 242); // #dde2f2
    let on_surface_variant = egui::Color32::from_rgb(192, 200, 203); // #c0c8cb
    let on_primary = egui::Color32::from_rgb(0, 54, 64); // #003640

    let outline_variant_40 = egui::Color32::from_rgba_premultiplied(64, 72, 75, 102); // 40% focus ghost border

    // Backgrounds
    visuals.window_fill = surface_container_low; // Panels/windows sit on surface base
    visuals.panel_fill = surface; // Base background
    visuals.extreme_bg_color = surface_container_lowest; // Deep recesses, text inputs
    visuals.faint_bg_color = surface_container_low;

    // No-Line Rule
    visuals.window_stroke = egui::Stroke::NONE;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.active.bg_stroke = egui::Stroke::NONE;

    // Widget colors & interactions
    visuals.widgets.noninteractive.bg_fill = surface_container_low;
    visuals.widgets.noninteractive.fg_stroke.color = on_surface_variant;

    visuals.widgets.inactive.bg_fill = surface_container; // Default cards/buttons
    visuals.widgets.inactive.fg_stroke.color = on_surface;

    visuals.widgets.hovered.bg_fill = surface_container_high;
    visuals.widgets.hovered.fg_stroke.color = on_surface;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, outline_variant_40); // Hover Ghost Border

    visuals.widgets.active.bg_fill = primary;
    visuals.widgets.active.fg_stroke.color = on_primary;

    visuals.selection.bg_fill = primary;
    visuals.selection.stroke.color = on_primary;

    // Rounding & Elevation
    let radius_md = 6.0; // Round-4 -> 6px for buttons/inputs
    let radius_xl = 12.0; // 12px for windows/cards
    visuals.widgets.noninteractive.rounding = egui::Rounding::same(radius_md);
    visuals.widgets.inactive.rounding = egui::Rounding::same(radius_md);
    visuals.widgets.hovered.rounding = egui::Rounding::same(radius_md);
    visuals.widgets.active.rounding = egui::Rounding::same(radius_md);
    visuals.window_rounding = egui::Rounding::same(radius_xl);

    // Ambient Shadows (Tinted with black at 40%)
    visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 24.0),
        blur: 48.0,
        spread: 0.0,
        color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 102),
    };
    visuals.popup_shadow = visuals.window_shadow;

    style.visuals = visuals;
    ctx.set_style(style);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Parsing,
    DryRun,
    Committing,
    Complete,
}

pub enum WorkerMessage {
    Progress(String),
    DryRunData(Vec<DryRunRow>),
    DryRunPrepared(ingestion::PendingIngestionCommit),
    Completed(ingestion::IngestionOutcome),
    Error(String),
}

pub struct NethericaApp {
    pub state: AppState,
    pub config: Config,
    pub selected_file: Option<PathBuf>,
    pub status_message: String,
    pub receiver: Option<mpsc::Receiver<WorkerMessage>>,
    pub dry_run_data: Vec<DryRunRow>,
    pub pending_commit: Option<ingestion::PendingIngestionCommit>,
    pub toast_message: Option<(String, std::time::Instant)>,
    pub critical_error: Option<String>,
    pub fallback_acknowledged: bool,
    pub last_report_path: Option<PathBuf>,
    pub post_generation_guidance: Option<String>,
    pub storage_fallback_warning_shown: bool,
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
        }
    }

    pub fn new(_cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        configure_egui_fonts(&_cc.egui_ctx);
        apply_design_system(&_cc.egui_ctx);

        let mut app = Self::from_config(config);

        if let Ok(data_dir) = DataDirectory::resolve() {
            app.maybe_show_storage_fallback_warning(&data_dir);
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

    fn render_idle_view(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.heading("File Ingestion");
            ui.label("Select an Excel file (.xlsx) to begin processing.");

            if ui.button("📁 Pick File").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Excel", &["xlsx"])
                    .pick_file()
                {
                    self.selected_file = Some(path.clone());
                    self.status_message =
                        format!("Selected: {:?}", path.file_name().unwrap_or_default());
                }
            }

            if let Some(path) = &self.selected_file {
                ui.label(format!("Selected: {}", path.display()));
                if ui.button("🚀 Start Ingestion").clicked() {
                    self.start_ingestion_worker(path.clone());
                }
            }
        });

        ui.add_space(20.0);

        ui.group(|ui| {
            ui.heading("Configuration Summary");
            ui.label(format!(
                "Products configured: {}",
                self.config.products.len()
            ));
            ui.label(format!(
                "Departments: {}",
                self.config
                    .departments
                    .iter()
                    .map(|(code, name)| format!("{} ({})", name, code))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            ui.label(format!("Database: {:?}", self.config.database_path));
        });
    }

    fn start_ingestion_worker(&mut self, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.state = AppState::Parsing;
        self.pending_commit = None;
        self.fallback_acknowledged = true;
        self.status_message = "Starting worker...".to_string();
        let config = self.config.clone();

        thread::spawn(move || {
            let _ = tx.send(WorkerMessage::Progress(
                "Initializing database...".to_string(),
            ));
            let outcome = (|| {
                let db = Database::new(&config.database_path)?;
                let repository = Repository::new(&db);

                let _ = tx.send(WorkerMessage::Progress(
                    "Parsing and preparing dry-run...".to_string(),
                ));
                ingestion::prepare_ingestion_dry_run(&path, &config, &repository)
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

    fn start_commit_worker(&mut self, pending: ingestion::PendingIngestionCommit) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.state = AppState::Committing;
        self.status_message = "Committing transaction...".to_string();
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

    fn render_dry_run_view(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.heading("Dry Run Preview");
            ui.label("Review Product + Department adjustment rows before committing.");
            ui.add_space(10.0);

            if let Some(pending) = &self.pending_commit {
                if pending.transaction_date_fallback_used {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "⚠ Some rows used file modification time (UTC) as transaction date fallback.",
                    );
                    if let Some(message) = &pending.transaction_date_warning {
                        ui.label(message);
                    }
                    ui.checkbox(
                        &mut self.fallback_acknowledged,
                        "I acknowledge this fallback and want to continue with commit.",
                    );
                    ui.add_space(10.0);
                }
            }

            let table_height = (ui.available_height() - 72.0).max(180.0);
            egui::ScrollArea::vertical()
                .max_height(table_height)
                .show(ui, |ui| {
                TableBuilder::new(ui)
                    .column(Column::remainder()) // Product
                    .column(Column::remainder()) // Department
                    .column(Column::auto()) // Opening Leftover
                    .column(Column::auto()) // Total Subunits Used
                    .column(Column::auto()) // Whole Units Output
                    .column(Column::auto()) // Closing Leftover
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("Product");
                        });
                        header.col(|ui| {
                            ui.strong("Department");
                        });
                        header.col(|ui| {
                            ui.strong("Opening Leftover");
                        });
                        header.col(|ui| {
                            ui.strong("Total Subunits Used (Product + Department)");
                        });
                        header.col(|ui| {
                            ui.strong("Whole Units Output");
                        });
                        header.col(|ui| {
                            ui.strong("Closing Leftover");
                        });
                    })
                    .body(|body| {
                        body.rows(20.0, self.dry_run_data.len(), |mut row| {
                            let index = row.index();
                            let row_data = &self.dry_run_data[index];
                            row.col(|ui| {
                                ui.label(format!(
                                    "{} ({})",
                                    row_data.product_display_name, row_data.product_id
                                ));
                            });
                            row.col(|ui| {
                                ui.label(format!(
                                    "{} ({})",
                                    row_data.department_display_name, row_data.department_id
                                ));
                            });
                            row.col(|ui| {
                                ui.label(row_data.opening_leftover.to_string());
                            });
                            row.col(|ui| {
                                ui.label(row_data.total_subunits_used.to_string());
                            });
                            row.col(|ui| {
                                ui.label(row_data.whole_units_output.to_string());
                            });
                            row.col(|ui| {
                                ui.label(row_data.closing_leftover.to_string());
                            });
                        });
                    });
                });

            ui.add_space(8.0);
            ui.label(format!(
                "{} adjustment row(s) (Product + Department).",
                self.dry_run_data.len()
            ));
            ui.separator();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.can_confirm_commit(), egui::Button::new("Confirm"))
                    .clicked()
                {
                    if let Some(pending) = self.pending_commit.take() {
                        self.start_commit_worker(pending);
                    } else {
                        self.status_message =
                            "No prepared ingestion payload found. Please start again.".to_string();
                        self.critical_error = Some(
                            "Missing prepared data for commit. Please re-run dry-run.".to_string(),
                        );
                        self.state = AppState::Idle;
                    }
                }
                if ui.button("Cancel").clicked() {
                    self.state = AppState::Idle;
                    self.dry_run_data.clear();
                    self.pending_commit = None;
                    self.fallback_acknowledged = true;
                }
            });
        });
    }

    fn process_worker_messages(&mut self) {
        if let Some(rx) = self.receiver.take() {
            let current_rx = rx;
            loop {
                match current_rx.try_recv() {
                    Ok(msg) => match msg {
                        WorkerMessage::Progress(progress) => {
                            self.status_message = progress;
                        }
                        WorkerMessage::DryRunData(data) => {
                            self.dry_run_data = data;
                            self.state = AppState::DryRun;
                        }
                        WorkerMessage::DryRunPrepared(prepared) => {
                            self.handle_dry_run_prepared(prepared);
                        }
                        WorkerMessage::Completed(outcome) => {
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
            let db = Database::new(&self.config.database_path)?;
            let repository = Repository::new(&db);
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

        egui::SidePanel::left("left_panel")
            .resizable(false)
            .exact_width(240.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading("Netherica");
                ui.heading("Pharmacy Reconciliation");
                ui.add_space(20.0);
                let _ = ui.selectable_label(true, "Ingestion");
                let _ = ui.selectable_label(false, "Reports");
                let _ = ui.selectable_label(false, "Inventory");
                let _ = ui.selectable_label(false, "Settings");
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            // Toast handling
            if let Some((msg, time)) = &self.toast_message {
                if time.elapsed() > std::time::Duration::from_secs(3) {
                    self.toast_message = None;
                } else {
                    egui::Area::new(egui::Id::new("toast"))
                        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -10.0))
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label(msg);
                            });
                        });
                }
            }

            // Error Modal
            let mut clear_error = false;
            if let Some(err) = &self.critical_error {
                egui::Window::new("Error")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(err);
                        if ui.button("Close").clicked() {
                            clear_error = true;
                        }
                    });
            }
            if clear_error {
                self.critical_error = None;
            }

            // Post-generation print guidance modal
            let mut clear_guidance = false;
            if let Some(message) = self.post_generation_guidance.clone() {
                egui::Window::new("Report Ready")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(&message);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Open Report").clicked() {
                                self.open_latest_report_action();
                            }
                            if ui.button("Open Report Folder").clicked() {
                                self.open_report_folder_action();
                            }
                            if ui.button("Close").clicked() {
                                clear_guidance = true;
                            }
                        });
                    });
            }
            if clear_guidance {
                self.post_generation_guidance = None;
            }

            ui.heading("Netherica v0.1 - Ingestion System");
            ui.add_space(10.0);

            match self.state {
                AppState::Idle => self.render_idle_view(ui),
                AppState::Parsing => {
                    ui.label("Parsing file...");
                    ui.add(egui::ProgressBar::new(0.5).animate(true));
                }
                AppState::DryRun => {
                    self.render_dry_run_view(ui);
                }
                AppState::Committing => {
                    ui.label("Committing to database...");
                    ui.add(egui::ProgressBar::new(0.5).animate(true));
                }
                AppState::Complete => {
                    ui.label("Process complete!");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("📄 Open Report Folder").clicked() {
                            self.open_report_folder_action();
                        }
                        if ui.button("🔄 Regenerate Last Report").clicked() {
                            self.regenerate_last_report();
                        }
                        if ui.button("📦 Retry Archive").clicked() {
                            match DataDirectory::resolve().and_then(|data_dir| {
                                ingestion::retry_pending_archive_moves(&data_dir.archive)
                            }) {
                                Ok(result) => {
                                    self.status_message = format!(
                                        "Archive retry complete: moved {}, pending {}",
                                        result.moved.len(),
                                        result.pending_count
                                    );
                                    self.toast_message = Some((
                                        self.status_message.clone(),
                                        std::time::Instant::now(),
                                    ));
                                }
                                Err(err) => {
                                    self.status_message = "Archive retry failed.".to_string();
                                    self.critical_error = Some(err.to_string());
                                }
                            }
                        }
                        if ui.button("✨ New File").clicked() {
                            self.state = AppState::Idle;
                            self.selected_file = None;
                            self.dry_run_data.clear();
                            self.pending_commit = None;
                            self.fallback_acknowledged = true;
                            self.post_generation_guidance = None;
                        }
                    });
                }
            }

            ui.add_space(20.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(format!("Status: {}", self.status_message));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{:?}", self.state));
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColumnNames, Settings};
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;

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
    fn font_definitions_register_noto_sans_thai_looped_with_fallbacks_preserved() {
        let fonts = build_font_definitions_with_utf8_support();

        assert!(fonts.font_data.contains_key(UI_THAI_FONT_NAME));

        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let family_fonts = fonts
                .families
                .get(&family)
                .expect("default family should exist");

            assert_eq!(
                family_fonts.first().map(String::as_str),
                Some(UI_THAI_FONT_NAME)
            );
            assert!(
                family_fonts.len() > 1,
                "default fallback fonts should remain available"
            );
        }
    }
}
