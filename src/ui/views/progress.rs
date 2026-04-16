use crate::ui::components::{format_duration_short, primary_button, surface_card_frame};
use crate::ui::theme::{
    CANVAS_GAP, CONTENT_HEIGHT, CONTENT_WIDTH, ERROR, ON_SURFACE, ON_SURFACE_VARIANT, PRIMARY,
    SURFACE_CONTAINER, SURFACE_CONTAINER_LOW, SURFACE_CONTAINER_LOWEST, TERTIARY,
};
use crate::ui::{AppState, NethericaApp};
use eframe::egui;

impl NethericaApp {
    pub(crate) fn render_parsing_view(&mut self, ui: &mut egui::Ui) {
        let (headline, subtext): (&str, String) = if self.state == AppState::ParsingHold {
            (
                "Parsing Complete",
                "Review parse summary and continue to dry run.".to_string(),
            )
        } else {
            let active_subtext = self
                .parsing_progress
                .as_ref()
                .map(|(sheet, _, _)| format!("Parsing sheet: {sheet}"))
                .unwrap_or_else(|| "Preparing parsing pipeline...".to_string());
            ("Analyzing Data Structure", active_subtext)
        };

        self.render_progress_shell(ui, headline, &subtext, false);
    }

    pub(crate) fn render_committing_view(&mut self, ui: &mut egui::Ui) {
        let subtext = if self.status_message.trim().is_empty() {
            "Committing prepared reconciliation transaction...".to_string()
        } else {
            self.status_message.clone()
        };
        self.render_progress_shell(ui, "Finalizing Reconciliation", &subtext, true);
    }

    fn render_progress_shell(
        &mut self,
        ui: &mut egui::Ui,
        headline: &str,
        subtext: &str,
        is_committing: bool,
    ) {
        let top_row_height = 280.0;
        let progress_width = 640.0;
        let metadata_width = CONTENT_WIDTH - progress_width - CANVAS_GAP;
        let log_height = (CONTENT_HEIGHT - top_row_height - CANVAS_GAP).max(180.0);

        ui.allocate_ui_with_layout(
            egui::vec2(CONTENT_WIDTH, top_row_height),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(progress_width, top_row_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        surface_card_frame(SURFACE_CONTAINER).show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(headline)
                                    .size(24.0)
                                    .strong()
                                    .color(ON_SURFACE),
                            );
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.colored_label(TERTIARY, "●");
                                ui.label(egui::RichText::new(subtext).color(ON_SURFACE_VARIANT));
                            });

                            ui.add_space(12.0);
                            if self.state == AppState::ParsingHold {
                                ui.add(
                                    egui::ProgressBar::new(1.0)
                                        .fill(PRIMARY)
                                        .show_percentage()
                                        .text("Parse complete"),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("100.0% complete")
                                        .color(ON_SURFACE_VARIANT),
                                );
                            } else if let Some((_, rows_processed, total_rows)) =
                                &self.parsing_progress
                            {
                                if *total_rows > 0 {
                                    let ratio = (*rows_processed as f32 / *total_rows as f32)
                                        .clamp(0.0, 1.0);
                                    ui.add(
                                        egui::ProgressBar::new(ratio)
                                            .fill(PRIMARY)
                                            .show_percentage()
                                            .text(format!("{rows_processed} / {total_rows} rows")),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{:.1}% complete",
                                            ratio * 100.0
                                        ))
                                        .color(ON_SURFACE_VARIANT),
                                    );
                                } else {
                                    ui.add(egui::ProgressBar::new(0.0).animate(true));
                                }
                            } else {
                                ui.add(egui::ProgressBar::new(0.0).animate(true));
                            }

                            if is_committing {
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(
                                        "Applying validated changes to the ledger...",
                                    )
                                    .color(ON_SURFACE_VARIANT),
                                );
                            }
                        });
                    },
                );

                ui.add_space(CANVAS_GAP);
                ui.allocate_ui_with_layout(
                    egui::vec2(metadata_width, top_row_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if self.state == AppState::ParsingHold {
                            let metadata_height = 158.0;
                            let right_gap = 8.0;
                            let action_height =
                                (top_row_height - metadata_height - right_gap).max(96.0);

                            ui.allocate_ui_with_layout(
                                egui::vec2(metadata_width, metadata_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    self.render_progress_metadata_card(ui, is_committing);
                                },
                            );
                            ui.add_space(right_gap);
                            ui.allocate_ui_with_layout(
                                egui::vec2(metadata_width, action_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    self.render_parsing_hold_actions(ui);
                                },
                            );
                        } else {
                            self.render_progress_metadata_card(ui, is_committing);
                        }
                    },
                );
            },
        );

        ui.add_space(CANVAS_GAP);
        ui.allocate_ui_with_layout(
            egui::vec2(CONTENT_WIDTH, log_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                surface_card_frame(SURFACE_CONTAINER_LOWEST).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("PARSING LOG")
                                .size(11.0)
                                .strong()
                                .color(ON_SURFACE_VARIANT),
                        );
                    });
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .max_height((log_height - 52.0).max(96.0))
                        .show(ui, |ui| {
                            if self.parsing_logs.is_empty() {
                                ui.label(
                                    egui::RichText::new("Waiting for parser logs...")
                                        .color(ON_SURFACE_VARIANT),
                                );
                            } else {
                                for (timestamp, level, message) in &self.parsing_logs {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "[{timestamp}] {level}: {message}"
                                        ))
                                        .color(log_level_color(level)),
                                    );
                                }
                            }
                        });
                });
            },
        );
    }

    fn render_progress_metadata_card(&self, ui: &mut egui::Ui, is_committing: bool) {
        surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
            ui.label(
                egui::RichText::new("FILE METADATA")
                    .size(11.0)
                    .strong()
                    .color(ON_SURFACE_VARIANT),
            );
            ui.add_space(8.0);

            if let Some(metadata) = &self.parsing_file_metadata {
                ui.label(
                    egui::RichText::new(&metadata.filename)
                        .strong()
                        .color(ON_SURFACE),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "Size: {} • Sheets: {}",
                        format_file_size(metadata.file_size),
                        metadata.sheet_count
                    ))
                    .color(ON_SURFACE_VARIANT),
                );
                ui.add_space(2.0);
                ui.add_sized(
                    [ui.available_width(), 0.0],
                    egui::Label::new(
                        egui::RichText::new(metadata.sheet_names.join(", "))
                            .color(ON_SURFACE_VARIANT),
                    )
                    .truncate(),
                );
            } else {
                ui.label(
                    egui::RichText::new("Workbook metadata will appear once parsing starts.")
                        .color(ON_SURFACE_VARIANT),
                );
            }

            if is_committing {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Committing prepared reconciliation transaction.")
                        .color(ON_SURFACE_VARIANT),
                );
            }
        });
    }

    fn render_parsing_hold_actions(&mut self, ui: &mut egui::Ui) {
        surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("PARSING SUMMARY")
                        .size(11.0)
                        .strong()
                        .color(ON_SURFACE_VARIANT),
                );
                let reviewed_rows = self.dry_run_data.len();
                ui.label(
                    egui::RichText::new(format!("Rows reviewed: {reviewed_rows}"))
                        .color(ON_SURFACE),
                );
                let elapsed = self
                    .dry_run_elapsed
                    .map(format_duration_short)
                    .unwrap_or_else(|| "Unavailable".to_string());
                ui.label(
                    egui::RichText::new(format!("Dry run preparation: {elapsed}"))
                        .color(ON_SURFACE_VARIANT),
                );

                ui.add_space(8.0);
                let continue_clicked = ui
                    .add_enabled_ui(self.can_continue_to_dry_run(), |ui| {
                        primary_button(ui, "Continue to Dry Run").clicked()
                    })
                    .inner;
                if continue_clicked {
                    self.finish_parsing_hold();
                }
            });
        });
    }
}

fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn log_level_color(level: &str) -> egui::Color32 {
    match level {
        "WARN" => TERTIARY,
        "ERROR" => ERROR,
        _ => ON_SURFACE_VARIANT,
    }
}
