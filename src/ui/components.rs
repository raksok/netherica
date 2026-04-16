use crate::domain::DryRunRow;
use crate::ui::theme;
use eframe::egui;

pub(crate) fn compute_warning_count(rows: &[DryRunRow]) -> usize {
    rows.iter()
        .filter(|row| row.closing_leftover < rust_decimal::Decimal::ZERO)
        .count()
}

#[allow(dead_code)] // Will be wired when views are redesigned in Step 6
pub(crate) fn compute_clean_count(rows: &[DryRunRow]) -> usize {
    rows.len().saturating_sub(compute_warning_count(rows))
}

pub(crate) fn truncate_hash(hash: &str) -> String {
    if hash.len() <= 12 {
        hash.to_string()
    } else {
        format!("{}...{}", &hash[..6], &hash[hash.len() - 4..])
    }
}

pub(crate) fn format_duration_short(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs_f64();
    if total_secs < 60.0 {
        format!("{:.1}s", total_secs)
    } else if total_secs < 3600.0 {
        let mins = (total_secs / 60.0) as u32;
        let secs = (total_secs % 60.0) as u32;
        format!("{mins}m {secs:02}s")
    } else {
        let hours = (total_secs / 3600.0) as u32;
        let mins = ((total_secs % 3600.0) / 60.0) as u32;
        format!("{hours}h {mins:02}m")
    }
}

pub(crate) fn primary_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(theme::ON_PRIMARY_CONTAINER);
        ui.add(
            egui::Button::new(text)
                .fill(theme::PRIMARY_CONTAINER)
                .stroke(egui::Stroke::NONE)
                .rounding(egui::Rounding::same(theme::RADIUS_MD))
                .min_size(egui::vec2(0.0, 32.0)),
        )
    })
    .inner
}

pub(crate) fn secondary_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(theme::ON_SURFACE);
        ui.add(
            egui::Button::new(text)
                .fill(theme::SURFACE_CONTAINER_HIGH)
                .stroke(egui::Stroke::NONE)
                .rounding(egui::Rounding::same(theme::RADIUS_MD))
                .min_size(egui::vec2(0.0, 32.0)),
        )
    })
    .inner
}

pub(crate) fn ghost_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(theme::ON_SURFACE_VARIANT);
        ui.add(
            egui::Button::new(text)
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE)
                .rounding(egui::Rounding::same(theme::RADIUS_MD))
                .min_size(egui::vec2(0.0, 32.0)),
        )
    })
    .inner
}

pub(crate) fn status_card(ui: &mut egui::Ui, label: &str, value: &str) {
    surface_card_frame(theme::SURFACE_CONTAINER_LOW).show(ui, |ui| {
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .size(11.0)
                .strong()
                .color(theme::ON_SURFACE_VARIANT),
        );
        ui.add_space(6.0);
        ui.label(egui::RichText::new(value).color(theme::ON_SURFACE));
    });
}

pub(crate) fn metric_card(ui: &mut egui::Ui, label: &str, value: &str) {
    surface_card_frame(theme::SURFACE_CONTAINER_LOW).show(ui, |ui| {
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .size(11.0)
                .strong()
                .color(theme::ON_SURFACE_VARIANT),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(value)
                .size(24.0)
                .strong()
                .color(theme::ON_SURFACE),
        );
    });
}

pub(crate) fn pill_chip(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let fill = if active {
        theme::SURFACE_CONTAINER_HIGH
    } else {
        egui::Color32::TRANSPARENT
    };
    let color = if active {
        theme::PRIMARY
    } else {
        theme::ON_SURFACE_VARIANT
    };

    ui.add(
        egui::Button::new(egui::RichText::new(text).color(color).size(11.0).strong())
            .fill(fill)
            .stroke(egui::Stroke::NONE)
            .rounding(egui::Rounding::same(theme::RADIUS_XL)),
    )
}

pub(crate) fn section_header(
    ui: &mut egui::Ui,
    eyebrow: Option<&str>,
    title: &str,
    body: Option<&str>,
) {
    if let Some(eyebrow) = eyebrow {
        ui.label(
            egui::RichText::new(eyebrow.to_uppercase())
                .size(11.0)
                .strong()
                .color(theme::PRIMARY),
        );
        ui.add_space(2.0);
    }

    ui.label(
        egui::RichText::new(title)
            .size(32.0)
            .strong()
            .color(theme::ON_SURFACE),
    );

    if let Some(body) = body {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(body).color(theme::ON_SURFACE_VARIANT));
    }
}

pub(crate) fn info_callout(ui: &mut egui::Ui, title: &str, body: &str) {
    surface_card_frame(theme::SURFACE_CONTAINER).show(ui, |ui| {
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .size(11.0)
                .strong()
                .color(theme::ON_SURFACE_VARIANT),
        );
        ui.add_space(4.0);
        ui.label(egui::RichText::new(body).color(theme::ON_SURFACE));
    });
}

pub(crate) fn surface_card_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .rounding(egui::Rounding::same(theme::RADIUS_XL))
        .inner_margin(egui::Margin::same(16.0))
}

pub(crate) fn overlay_card_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .rounding(egui::Rounding::same(theme::RADIUS_XL))
        .inner_margin(egui::Margin::same(20.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 24.0),
            blur: 48.0,
            spread: 0.0,
            color: theme::SHADOW_COLOR,
        })
}
