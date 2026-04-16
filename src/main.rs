pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod ingestion;
pub mod logging;
pub mod models;
pub mod report;
pub mod repository;
pub mod storage;
pub mod ui;

use error::AppResult;
use ui::theme;
use ui::NethericaApp;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> AppResult<()> {
    // Load configuration first to get the database path for logging
    let config = crate::config::Config::load()?;

    // Initialize logging
    crate::logging::init_logging(&config.database_path)?;

    // Initialize eframe
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Netherica | Pharmacy Reconciliation")
            .with_inner_size([theme::FIXED_WINDOW_WIDTH, theme::FIXED_WINDOW_HEIGHT])
            .with_min_inner_size([theme::FIXED_WINDOW_WIDTH, theme::FIXED_WINDOW_HEIGHT])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Netherica | Pharmacy Reconciliation",
        native_options,
        Box::new(move |cc| {
            // Initialize the app state
            Ok(Box::new(NethericaApp::new(cc, config)))
        }),
    )
    .map_err(|e| crate::error::AppError::InternalError(e.to_string()))?;

    Ok(())
}
