use crate::ui::components::{
    ghost_button, info_callout, primary_button, secondary_button, section_header,
    surface_card_frame,
};
use crate::ui::theme::{
    CANVAS_GAP, CONTENT_WIDTH, ON_SURFACE_VARIANT, PRIMARY, SURFACE_CONTAINER_LOW,
};
use crate::ui::NethericaApp;
use eframe::egui;

impl NethericaApp {
    pub(crate) fn render_reports_view(&mut self, ui: &mut egui::Ui) {
        section_header(
            ui,
            Some("Analytics"),
            "Reports",
            Some("Generated reconciliation reports from completed ingestion runs."),
        );
        ui.add_space(CANVAS_GAP);

        info_callout(
            ui,
            "Note",
            "Report browsing and history UI are planned for a future release. Use the actions below to access your most recent report.",
        );
        ui.add_space(CANVAS_GAP);

        if let Some(ref path) = self.last_report_path {
            ui.allocate_ui_with_layout(
                egui::vec2(CONTENT_WIDTH, 180.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    render_latest_report_context(ui, path);
                },
            );
            ui.add_space(CANVAS_GAP);
        }

        surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let has_report = self.last_report_path.is_some();

                ui.add_enabled_ui(has_report, |ui| {
                    if primary_button(ui, "Open Latest Report").clicked() {
                        self.open_latest_report_action();
                    }
                });

                if secondary_button(ui, "Open Report Folder").clicked() {
                    self.open_report_folder_action();
                }

                if has_report && ghost_button(ui, "Regenerate Last Report").clicked() {
                    self.regenerate_last_report();
                }
            });
        });
    }
}

fn render_latest_report_context(ui: &mut egui::Ui, path: &std::path::Path) {
    surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
        ui.label(
            egui::RichText::new("LATEST REPORT")
                .size(11.0)
                .strong()
                .color(ON_SURFACE_VARIANT),
        );
        ui.add_space(6.0);

        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            ui.label(
                egui::RichText::new(format!("File: {filename}"))
                    .color(PRIMARY)
                    .strong(),
            );
        }

        if let Some(parent) = path.parent().and_then(|p| p.to_str()) {
            ui.label(egui::RichText::new(format!("Folder: {parent}")).color(ON_SURFACE_VARIANT));
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Opens in your system default browser.")
                .size(11.0)
                .color(ON_SURFACE_VARIANT),
        );
    });
}
