use crate::ingestion;
use crate::storage::DataDirectory;
use crate::ui::components::{
    format_duration_short, ghost_button, primary_button, secondary_button, section_header,
    surface_card_frame, truncate_hash,
};
use crate::ui::theme::{
    CANVAS_GAP, CONTENT_WIDTH, ON_SURFACE, ON_SURFACE_VARIANT, PRIMARY, SURFACE_CONTAINER,
    SURFACE_CONTAINER_LOW, SURFACE_CONTAINER_LOWEST, TERTIARY,
};
use crate::ui::AppState;
use crate::ui::NethericaApp;
use crate::APP_VERSION;
use eframe::egui;

impl NethericaApp {
    pub(crate) fn render_complete_view(&mut self, ui: &mut egui::Ui) {
        section_header(ui, Some("Step 04"), "Process Completion", None);
        ui.add_space(12.0);

        let hero_width = 640.0;
        let action_width = CONTENT_WIDTH - hero_width - CANVAS_GAP;

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(hero_width, 360.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("●").size(44.0).color(PRIMARY));
                            ui.label(
                                egui::RichText::new("Reconciliation Successful")
                                    .size(32.0)
                                    .strong()
                                    .color(PRIMARY),
                            );
                            ui.add_space(6.0);

                            let filename = if self.completed_filename.is_empty() {
                                "Unavailable"
                            } else {
                                &self.completed_filename
                            };
                            ui.label(
                                egui::RichText::new(format!("File: {filename}"))
                                    .color(ON_SURFACE_VARIANT),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "Rows processed: {}",
                                    self.completed_rows_processed
                                ))
                                .color(ON_SURFACE),
                            );
                        });

                        if self.completed_archive_move_pending {
                            ui.add_space(10.0);
                            surface_card_frame(SURFACE_CONTAINER_LOWEST).show(ui, |ui| {
                                ui.label(
                                egui::RichText::new(
                                    "Archive move is pending. You can retry archive now or later.",
                                )
                                .strong()
                                .color(TERTIARY),
                            );
                            });
                        }
                    });
                },
            );

            ui.add_space(CANVAS_GAP);
            ui.allocate_ui_with_layout(
                egui::vec2(action_width, 360.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    surface_card_frame(SURFACE_CONTAINER).show(ui, |ui| {
                        if secondary_button(ui, "Open Report Folder").clicked() {
                            self.open_report_folder_action();
                        }
                        ui.add_space(8.0);
                        if secondary_button(ui, "Regenerate Last Report").clicked() {
                            self.regenerate_last_report();
                        }
                        ui.add_space(8.0);
                        if ghost_button(ui, "Retry Archive").clicked() {
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
                        ui.add_space(8.0);
                        if primary_button(ui, "New File").clicked() {
                            self.state = AppState::Idle;
                            self.selected_file = None;
                            self.dry_run_data.clear();
                            self.pending_commit = None;
                            self.fallback_acknowledged = true;
                            self.post_generation_guidance = None;
                            self.clear_parsing_state();
                            self.clear_completion_state();
                        }
                    });
                },
            );
        });

        ui.add_space(CANVAS_GAP);
        surface_card_frame(SURFACE_CONTAINER_LOWEST).show(ui, |ui| {
            let execution_time = self
                .pipeline_start
                .map(|start| format_duration_short(start.elapsed()))
                .unwrap_or_else(|| "Unavailable".to_string());
            let archive_status = if self.completed_archive_move_pending {
                "Pending"
            } else {
                "Completed"
            };
            let hash = if self.completed_file_hash.is_empty() {
                "Unavailable".to_string()
            } else {
                truncate_hash(&self.completed_file_hash)
            };

            ui.label(
                egui::RichText::new("SYSTEM HEALTH & METADATA")
                    .size(11.0)
                    .strong()
                    .color(ON_SURFACE_VARIANT),
            );
            ui.add_space(8.0);
            ui.columns(4, |columns| {
                metadata_cell(&mut columns[0], "Execution Time", &execution_time);
                metadata_cell(&mut columns[1], "Archive Status", archive_status);
                metadata_cell(&mut columns[2], "Validator Version", APP_VERSION);
                metadata_cell(&mut columns[3], "File Hash", &hash);
            });
        });
    }
}

fn metadata_cell(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label.to_uppercase())
            .size(11.0)
            .strong()
            .color(ON_SURFACE_VARIANT),
    );
    ui.add_space(2.0);
    ui.label(egui::RichText::new(value).strong().color(ON_SURFACE));
}
