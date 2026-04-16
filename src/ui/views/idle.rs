use crate::ui::components::{primary_button, status_card, surface_card_frame};
use crate::ui::theme::{
    CANVAS_GAP, CONTENT_HEIGHT, CONTENT_WIDTH, ON_SURFACE, ON_SURFACE_VARIANT, PRIMARY,
    SURFACE_CONTAINER, SURFACE_CONTAINER_LOW,
};
use crate::ui::NethericaApp;
use eframe::egui;

impl NethericaApp {
    pub(crate) fn render_idle_view(&mut self, ui: &mut egui::Ui) {
        let hero_width = 632.0;
        let summary_width = 328.0;
        let status_card_width = (CONTENT_WIDTH - (CANVAS_GAP * 2.0)) / 3.0;

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(hero_width, 360.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                surface_card_frame(SURFACE_CONTAINER).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Ready for new reconciliation run?")
                            .size(32.0)
                            .strong()
                            .color(ON_SURFACE),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Upload your latest Pharmacy Excel export to begin the automated discrepancy audit.",
                        )
                        .color(ON_SURFACE_VARIANT),
                    );
                    ui.add_space(20.0);

                    if primary_button(ui, "Select Excel File").clicked() {
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
                        ui.add_space(10.0);
                        let selected_name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        ui.label(
                            egui::RichText::new(format!("Selected: {selected_name}"))
                                .color(ON_SURFACE_VARIANT),
                        );
                        ui.add_space(8.0);
                        if primary_button(ui, "Start Ingestion").clicked() {
                            self.start_ingestion_worker(path.clone());
                        }
                    }
                });
                },
            );

            ui.add_space(CANVAS_GAP);
            ui.allocate_ui_with_layout(
                egui::vec2(summary_width, 360.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("CURRENT CONFIGURATION")
                            .size(11.0)
                            .strong()
                            .color(ON_SURFACE_VARIANT),
                    );
                    ui.add_space(14.0);

                    ui.label(
                        egui::RichText::new(self.config.products.len().to_string())
                            .size(32.0)
                            .strong()
                            .color(PRIMARY),
                    );
                    ui.label(egui::RichText::new("Active Products").color(ON_SURFACE_VARIANT));
                    ui.add_space(10.0);

                    ui.label(
                        egui::RichText::new(self.config.departments.len().to_string())
                            .size(32.0)
                            .strong()
                            .color(PRIMARY),
                    );
                    ui.label(egui::RichText::new("Departments").color(ON_SURFACE_VARIANT));
                    ui.add_space(10.0);

                    let chronology_status = if self.config.settings.strict_chronological {
                        "Strict chronology enabled"
                    } else {
                        "Strict chronology disabled"
                    };
                    ui.label(
                        egui::RichText::new(chronology_status)
                            .size(11.0)
                            .strong()
                            .color(ON_SURFACE),
                    );
                });
                },
            );
        });

        ui.add_space(CANVAS_GAP);
        ui.horizontal_top(|ui| {
            let last_run = self
                .last_run_timestamp
                .map(|ts| ts.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| "No files processed".to_string());
            ui.allocate_ui_with_layout(
                egui::vec2(status_card_width, CONTENT_HEIGHT - 360.0 - CANVAS_GAP),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    status_card(ui, "Last Run", &last_run);
                },
            );

            let source_text = self
                .storage_source
                .map(|source| format!(" • {}", format_storage_source(source)))
                .unwrap_or_default();
            let sync_state = if self.db_connected {
                format!("Database connected{source_text}")
            } else {
                format!("Database unavailable{source_text}")
            };
            ui.add_space(CANVAS_GAP);
            ui.allocate_ui_with_layout(
                egui::vec2(status_card_width, CONTENT_HEIGHT - 360.0 - CANVAS_GAP),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    status_card(ui, "Sync State", &sync_state);
                },
            );

            let config_status = format!(
                "{} products • {} departments",
                self.config.products.len(),
                self.config.departments.len()
            );
            ui.add_space(CANVAS_GAP);
            ui.allocate_ui_with_layout(
                egui::vec2(status_card_width, CONTENT_HEIGHT - 360.0 - CANVAS_GAP),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    status_card(ui, "Config Status", &config_status);
                },
            );
        });
    }
}

fn format_storage_source(source: crate::storage::DataRootSource) -> &'static str {
    match source {
        crate::storage::DataRootSource::ExecutableDirectory => "Executable storage",
        crate::storage::DataRootSource::OsUserDataFallback => "OS fallback storage",
    }
}
