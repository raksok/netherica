use crate::config::ProductConfig;
use crate::ui::components::{pill_chip, section_header, surface_card_frame};
use crate::ui::theme::{ON_SURFACE, ON_SURFACE_VARIANT, PRIMARY, SURFACE_CONTAINER_LOW};
use crate::ui::{NethericaApp, SettingsTab};
use eframe::egui;

impl NethericaApp {
    pub(crate) fn render_settings_view(&mut self, ui: &mut egui::Ui) {
        section_header(
            ui,
            Some("Configuration"),
            "Settings",
            Some("Read-only view of current application configuration."),
        );
        ui.add_space(16.0);
        self.render_settings_tab_bar(ui);
        ui.add_space(16.0);

        match self.active_settings_tab {
            SettingsTab::Departments => self.render_departments_view(ui),
            SettingsTab::Products => self.render_products_view(ui),
        }
    }

    fn render_settings_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if pill_chip(
                ui,
                "Departments",
                self.active_settings_tab == SettingsTab::Departments,
            )
            .clicked()
            {
                self.active_settings_tab = SettingsTab::Departments;
            }
            if pill_chip(
                ui,
                "Products",
                self.active_settings_tab == SettingsTab::Products,
            )
            .clicked()
            {
                self.active_settings_tab = SettingsTab::Products;
            }
        });
    }

    fn render_departments_view(&mut self, ui: &mut egui::Ui) {
        if self.config.departments.is_empty() {
            ui.label(egui::RichText::new("No departments configured.").color(ON_SURFACE_VARIANT));
            return;
        }

        let cards: Vec<(&String, &String)> = self.config.departments.iter().collect();
        let columns = 3;
        let rows = cards.len().div_ceil(columns);

        for row_idx in 0..rows {
            ui.horizontal(|ui| {
                for col_idx in 0..columns {
                    let card_idx = row_idx * columns + col_idx;
                    if let Some((code, name)) = cards.get(card_idx) {
                        ui.vertical(|ui| {
                            render_department_card(ui, code, name);
                        });
                    }
                }
            });
            ui.add_space(8.0);
        }
    }

    fn render_products_view(&mut self, ui: &mut egui::Ui) {
        if self.config.products.is_empty() {
            ui.label(egui::RichText::new("No products configured.").color(ON_SURFACE_VARIANT));
            return;
        }

        let columns = 3;
        let rows = self.config.products.len().div_ceil(columns);

        for row_idx in 0..rows {
            ui.horizontal(|ui| {
                for col_idx in 0..columns {
                    let card_idx = row_idx * columns + col_idx;
                    if let Some(product) = self.config.products.get(card_idx) {
                        ui.vertical(|ui| {
                            render_product_card(ui, product);
                        });
                    }
                }
            });
            ui.add_space(8.0);
        }
    }
}

fn render_department_card(ui: &mut egui::Ui, department_code: &str, department_name: &str) {
    surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
        ui.set_min_size(egui::vec2(220.0, 0.0));
        ui.label(
            egui::RichText::new("DEPARTMENT CODE")
                .size(11.0)
                .strong()
                .color(ON_SURFACE_VARIANT),
        );
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(department_code)
                .size(15.0)
                .strong()
                .color(PRIMARY),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("MAPPED DISPLAY NAME")
                .size(11.0)
                .strong()
                .color(ON_SURFACE_VARIANT),
        );
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(department_name)
                .size(14.0)
                .strong()
                .color(ON_SURFACE),
        );
    });
}

fn render_product_card(ui: &mut egui::Ui, product: &ProductConfig) {
    surface_card_frame(SURFACE_CONTAINER_LOW).show(ui, |ui| {
        ui.set_min_size(egui::vec2(220.0, 0.0));

        ui.label(
            egui::RichText::new(format!("ID: {}", product.id))
                .size(11.0)
                .strong()
                .color(PRIMARY),
        );
        ui.add_space(2.0);
        let display_name = truncate_display_name(&product.display_name, 42);
        ui.label(
            egui::RichText::new(display_name)
                .size(14.0)
                .strong()
                .color(ON_SURFACE),
        )
        .on_hover_text(&product.display_name);
        ui.add_space(12.0);

        render_detail_pair(ui, "Unit", &product.unit);
        render_detail_pair(ui, "Subunit", &product.subunit);
        render_detail_pair(ui, "Factor", &product.factor.to_string());
        render_detail_pair(
            ui,
            "Track subunits",
            if product.track_subunits { "Yes" } else { "No" },
        );
    });
}

fn truncate_display_name(value: &str, max_chars: usize) -> String {
    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return value.to_string();
    }

    let truncated: String = value.chars().take(max_chars).collect();
    format!("{truncated}…")
}

fn render_detail_pair(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}:", label))
                .size(11.0)
                .color(ON_SURFACE_VARIANT),
        );
        ui.label(egui::RichText::new(value).size(11.0).color(ON_SURFACE));
    });
}
