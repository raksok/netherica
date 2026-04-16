use crate::ui::theme;
use crate::ui::{NavigationSection, NethericaApp};
use eframe::egui;

impl NethericaApp {
    pub(crate) fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("left_panel")
            .resizable(false)
            .exact_width(theme::SIDEBAR_WIDTH)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Netherica")
                        .size(20.0)
                        .strong()
                        .color(theme::ON_SURFACE),
                );
                ui.label(
                    egui::RichText::new("Pharmacy Reconciliation")
                        .size(12.0)
                        .color(theme::ON_SURFACE_VARIANT),
                );
                ui.add_space(20.0);

                let sections = [
                    ("Ingestion", NavigationSection::Ingestion),
                    ("Reports", NavigationSection::Reports),
                    ("Settings", NavigationSection::Settings),
                ];

                for (label, section) in sections {
                    let is_active = self.active_section == section;
                    if nav_item(ui, label, is_active).clicked() {
                        self.active_section = section;
                    }
                }
            });
    }
}

fn nav_item(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let fill = if active {
        crate::ui::theme::SURFACE_CONTAINER_HIGH
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if active {
        crate::ui::theme::PRIMARY
    } else {
        crate::ui::theme::ON_SURFACE_VARIANT
    };
    ui.scope(|ui| {
        ui.add(
            egui::Button::new(egui::RichText::new(label).color(text_color).size(13.0))
                .fill(fill)
                .stroke(egui::Stroke::NONE)
                .rounding(egui::Rounding::same(crate::ui::theme::RADIUS_MD)),
        )
    })
    .inner
}
