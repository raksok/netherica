use crate::ui::components::{
    compute_warning_count, format_duration_short, ghost_button, metric_card, primary_button,
    section_header, surface_card_frame,
};
use crate::ui::theme::{
    CANVAS_GAP, CONTENT_HEIGHT, CONTENT_WIDTH, ON_SURFACE, ON_SURFACE_VARIANT, SURFACE_CONTAINER,
    SURFACE_CONTAINER_LOW, TERTIARY,
};
use crate::ui::NethericaApp;
use eframe::egui;
use egui_extras::{Column, TableBuilder};

impl NethericaApp {
    pub(crate) fn render_dry_run_view(&mut self, ui: &mut egui::Ui) {
        section_header(
            ui,
            None,
            "Dry Run Review",
            Some("Validate reconciliation metrics before committing."),
        );
        ui.add_space(8.0);

        let rows_reviewed = self.dry_run_data.len();
        let warning_count = compute_warning_count(&self.dry_run_data);
        let dry_run_duration = self
            .dry_run_elapsed
            .map(format_duration_short)
            .unwrap_or_else(|| "Unavailable".to_string());

        let metric_width = (CONTENT_WIDTH - (CANVAS_GAP * 2.0)) / 3.0;

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(metric_width, 96.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    metric_card(ui, "Rows Reviewed", &rows_reviewed.to_string());
                },
            );
            ui.add_space(CANVAS_GAP);
            ui.allocate_ui_with_layout(
                egui::vec2(metric_width, 96.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    surface_card_frame(SURFACE_CONTAINER).show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("WARNINGS")
                                .size(11.0)
                                .strong()
                                .color(ON_SURFACE_VARIANT),
                        );
                        ui.add_space(6.0);
                        let warning_color = if warning_count > 0 {
                            TERTIARY
                        } else {
                            ON_SURFACE
                        };
                        ui.label(
                            egui::RichText::new(warning_count.to_string())
                                .size(24.0)
                                .strong()
                                .color(warning_color),
                        );
                    });
                },
            );
            ui.add_space(CANVAS_GAP);
            ui.allocate_ui_with_layout(
                egui::vec2(metric_width, 96.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    metric_card(ui, "Dry Run Duration", &dry_run_duration);
                },
            );
        });

        ui.add_space(10.0);
        ui.allocate_ui_with_layout(
            egui::vec2(CONTENT_WIDTH, 120.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("COMMIT ACTIONS")
                                .size(11.0)
                                .strong()
                                .color(ON_SURFACE_VARIANT),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Confirming will finalize the period-end reconciliation ledger.",
                            )
                            .color(ON_SURFACE_VARIANT),
                        );

                        if let Some(pending) = &self.pending_commit {
                            if pending.transaction_date_fallback_used {
                                ui.add_space(6.0);
                                ui.colored_label(
                                    TERTIARY,
                                    "⚠ Some rows used file modification time (UTC) as transaction date fallback.",
                                );
                                if let Some(message) = &pending.transaction_date_warning {
                                    ui.label(egui::RichText::new(message).color(ON_SURFACE_VARIANT));
                                }
                                ui.checkbox(
                                    &mut self.fallback_acknowledged,
                                    "I acknowledge this fallback and want to continue with commit.",
                                );
                            }
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let confirm_clicked = ui
                            .add_enabled_ui(self.can_confirm_commit(), |ui| {
                                primary_button(ui, "Confirm & Generate Report").clicked()
                            })
                            .inner;
                        if confirm_clicked {
                            if let Some(pending) = self.pending_commit.take() {
                                self.start_commit_worker(pending);
                            } else {
                                self.status_message =
                                    "No prepared ingestion payload found. Please start again."
                                        .to_string();
                                self.critical_error = Some(
                                    "Missing prepared data for commit. Please re-run dry-run."
                                        .to_string(),
                                );
                                self.state = crate::ui::AppState::Idle;
                            }
                        }

                        if ghost_button(ui, "Cancel").clicked() {
                            self.state = crate::ui::AppState::Idle;
                            self.dry_run_data.clear();
                            self.pending_commit = None;
                            self.fallback_acknowledged = true;
                            self.clear_parsing_state();
                        }
                    });
                });
            });
            },
        );

        ui.add_space(10.0);
        let table_height = (CONTENT_HEIGHT - 96.0 - 120.0 - 80.0).max(220.0);
        ui.allocate_ui_with_layout(
            egui::vec2(CONTENT_WIDTH, table_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                surface_card_frame(SURFACE_CONTAINER).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((table_height - 34.0).max(120.0))
                        .show(ui, |ui| {
                            TableBuilder::new(ui)
                                .column(Column::exact(220.0))
                                .column(Column::exact(190.0))
                                .column(Column::exact(110.0))
                                .column(Column::exact(160.0))
                                .column(Column::exact(110.0))
                                .column(Column::remainder())
                                .header(24.0, |mut header| {
                                    header.col(|ui| {
                                        paint_header_background(ui);
                                        ui.strong("Product");
                                    });
                                    header.col(|ui| {
                                        paint_header_background(ui);
                                        ui.strong("Department");
                                    });
                                    header.col(|ui| {
                                        paint_header_background(ui);
                                        ui.strong("Opening");
                                    });
                                    header.col(|ui| {
                                        paint_header_background(ui);
                                        ui.strong("Used");
                                    });
                                    header.col(|ui| {
                                        paint_header_background(ui);
                                        ui.strong("Whole");
                                    });
                                    header.col(|ui| {
                                        paint_header_background(ui);
                                        ui.strong("Closing");
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
                                                row_data.department_display_name,
                                                row_data.department_id
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
                });
            },
        );

        ui.add_space(6.0);
        ui.label(format!(
            "{} adjustment row(s) (Product + Department).",
            self.dry_run_data.len()
        ));
    }
}

fn paint_header_background(ui: &egui::Ui) {
    ui.painter().rect_filled(
        ui.max_rect(),
        egui::Rounding::same(0.0),
        SURFACE_CONTAINER_LOW,
    );
}
