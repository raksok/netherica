use eframe::egui;

pub(crate) const FIXED_WINDOW_WIDTH: f32 = 1280.0;
pub(crate) const FIXED_WINDOW_HEIGHT: f32 = 800.0;
pub(crate) const SIDEBAR_WIDTH: f32 = 256.0;
pub(crate) const STATUS_BAR_HEIGHT: f32 = 36.0;
pub(crate) const CANVAS_PADDING: f32 = 24.0;
pub(crate) const CANVAS_GAP: f32 = 16.0;
pub(crate) const MAIN_CANVAS_WIDTH: f32 = FIXED_WINDOW_WIDTH - SIDEBAR_WIDTH;
pub(crate) const MAIN_CANVAS_HEIGHT: f32 = FIXED_WINDOW_HEIGHT - STATUS_BAR_HEIGHT;
pub(crate) const CONTENT_WIDTH: f32 = MAIN_CANVAS_WIDTH - (CANVAS_PADDING * 2.0);
pub(crate) const CONTENT_HEIGHT: f32 = MAIN_CANVAS_HEIGHT - (CANVAS_PADDING * 2.0);

pub(crate) const UI_PRIMARY_FONT_NAME: &str = "inter_variable";
pub(crate) const UI_PRIMARY_FONT_BYTES: &[u8] =
    include_bytes!("../../asset/fonts/Inter/Inter-VariableFont_opsz,wght.ttf");
pub(crate) const UI_THAI_FONT_NAME: &str = "noto_sans_thai_looped_regular";
pub(crate) const UI_THAI_FONT_BYTES: &[u8] =
    include_bytes!("../../asset/fonts/NotoSansThaiLooped/NotoSansThaiLooped-Regular.ttf");

pub(crate) const SURFACE: egui::Color32 = egui::Color32::from_rgb(13, 19, 30);
pub(crate) const SURFACE_CONTAINER_LOWEST: egui::Color32 = egui::Color32::from_rgb(8, 14, 25);
pub(crate) const SURFACE_CONTAINER_LOW: egui::Color32 = egui::Color32::from_rgb(22, 28, 39);
pub(crate) const SURFACE_CONTAINER: egui::Color32 = egui::Color32::from_rgb(26, 32, 43);
pub(crate) const SURFACE_CONTAINER_HIGH: egui::Color32 = egui::Color32::from_rgb(36, 42, 54);
pub(crate) const SURFACE_CONTAINER_HIGHEST: egui::Color32 = egui::Color32::from_rgb(47, 53, 65);
#[allow(dead_code)]
pub(crate) const SURFACE_VARIANT: egui::Color32 = egui::Color32::from_rgb(47, 53, 65);
#[allow(dead_code)]
pub(crate) const SURFACE_BRIGHT: egui::Color32 = egui::Color32::from_rgb(51, 57, 69);

pub(crate) const PRIMARY: egui::Color32 = egui::Color32::from_rgb(163, 220, 236);
pub(crate) const PRIMARY_CONTAINER: egui::Color32 = egui::Color32::from_rgb(136, 192, 208);
#[allow(dead_code)]
pub(crate) const SECONDARY: egui::Color32 = egui::Color32::from_rgb(169, 202, 235);
pub(crate) const TERTIARY: egui::Color32 = egui::Color32::from_rgb(240, 207, 143);
pub(crate) const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 180, 171);

pub(crate) const ON_SURFACE: egui::Color32 = egui::Color32::from_rgb(221, 226, 242);
pub(crate) const ON_SURFACE_VARIANT: egui::Color32 = egui::Color32::from_rgb(192, 200, 203);
pub(crate) const ON_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0, 54, 64);
#[allow(dead_code)]
pub(crate) const OUTLINE: egui::Color32 = egui::Color32::from_rgb(138, 146, 149);
#[allow(dead_code)]
pub(crate) const OUTLINE_VARIANT: egui::Color32 = egui::Color32::from_rgb(64, 72, 75);

pub(crate) const ON_PRIMARY_CONTAINER: egui::Color32 = egui::Color32::from_rgb(12, 79, 93);
#[allow(dead_code)]
pub(crate) const INVERSE_SURFACE: egui::Color32 = egui::Color32::from_rgb(221, 226, 242);
#[allow(dead_code)]
pub(crate) const INVERSE_ON_SURFACE: egui::Color32 = egui::Color32::from_rgb(43, 49, 60);
#[allow(dead_code)]
pub(crate) const INVERSE_PRIMARY: egui::Color32 = egui::Color32::from_rgb(43, 102, 116);
#[allow(dead_code)]
pub(crate) const SURFACE_TINT: egui::Color32 = egui::Color32::from_rgb(151, 207, 224);

pub(crate) const RADIUS_MD: f32 = 6.0;
pub(crate) const RADIUS_XL: f32 = 12.0;
pub(crate) const SHADOW_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 102);
pub(crate) const MODAL_OVERLAY: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(0, 0, 0, 166);
pub(crate) const GHOST_BORDER_FOCUS: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(64, 72, 75, 102);

pub(crate) fn build_font_definitions_with_utf8_support() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        UI_PRIMARY_FONT_NAME.to_string(),
        egui::FontData::from_static(UI_PRIMARY_FONT_BYTES),
    );
    fonts.font_data.insert(
        UI_THAI_FONT_NAME.to_string(),
        egui::FontData::from_static(UI_THAI_FONT_BYTES),
    );

    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    proportional.retain(|font| font != UI_PRIMARY_FONT_NAME && font != UI_THAI_FONT_NAME);
    proportional.insert(0, UI_THAI_FONT_NAME.to_string());
    proportional.insert(0, UI_PRIMARY_FONT_NAME.to_string());

    let monospace = fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default();
    monospace.retain(|font| font != UI_THAI_FONT_NAME);
    monospace.push(UI_THAI_FONT_NAME.to_string());

    fonts
}

pub(crate) fn configure_egui_fonts(ctx: &egui::Context) {
    ctx.set_fonts(build_font_definitions_with_utf8_support());
}

pub(crate) fn apply_design_system(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();

    // 1. Spacing & Layout
    // Spacing scale 2 (base 4px). item_spacing = space-3 (12px), window_margin = space-6 (24px)
    style.spacing.item_spacing = egui::vec2(12.0, 12.0);
    style.spacing.window_margin = egui::Margin::same(24.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);

    // Backgrounds
    visuals.window_fill = SURFACE_CONTAINER_LOW; // Panels/windows sit on surface base
    visuals.panel_fill = SURFACE; // Base background
    visuals.extreme_bg_color = SURFACE_CONTAINER_LOWEST; // Deep recesses, text inputs
    visuals.faint_bg_color = SURFACE_CONTAINER_LOW;

    // No-Line Rule
    visuals.window_stroke = egui::Stroke::NONE;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.active.bg_stroke = egui::Stroke::NONE;

    // Widget colors & interactions
    visuals.widgets.noninteractive.bg_fill = SURFACE_CONTAINER_LOW;
    visuals.widgets.noninteractive.fg_stroke.color = ON_SURFACE_VARIANT;

    visuals.widgets.inactive.bg_fill = SURFACE_CONTAINER; // Default cards/buttons
    visuals.widgets.inactive.fg_stroke.color = ON_SURFACE;

    visuals.widgets.hovered.bg_fill = SURFACE_CONTAINER_HIGH;
    visuals.widgets.hovered.fg_stroke.color = ON_SURFACE;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, GHOST_BORDER_FOCUS); // Hover Ghost Border

    visuals.widgets.active.bg_fill = PRIMARY;
    visuals.widgets.active.fg_stroke.color = ON_PRIMARY;

    visuals.selection.bg_fill = PRIMARY;
    visuals.selection.stroke.color = ON_PRIMARY;

    // Rounding & Elevation
    visuals.widgets.noninteractive.rounding = egui::Rounding::same(RADIUS_MD);
    visuals.widgets.inactive.rounding = egui::Rounding::same(RADIUS_MD);
    visuals.widgets.hovered.rounding = egui::Rounding::same(RADIUS_MD);
    visuals.widgets.active.rounding = egui::Rounding::same(RADIUS_MD);
    visuals.window_rounding = egui::Rounding::same(RADIUS_XL);

    // Ambient Shadows (Tinted with black at 40%)
    visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 24.0),
        blur: 48.0,
        spread: 0.0,
        color: SHADOW_COLOR,
    };
    visuals.popup_shadow = visuals.window_shadow;

    style.visuals = visuals;
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_definitions_register_ui_primary_and_thai_fallback_fonts() {
        let fonts = build_font_definitions_with_utf8_support();

        assert!(fonts.font_data.contains_key(UI_PRIMARY_FONT_NAME));
        assert!(fonts.font_data.contains_key(UI_THAI_FONT_NAME));

        let proportional_fonts = fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .expect("proportional family should exist");
        assert_eq!(
            proportional_fonts.first().map(String::as_str),
            Some(UI_PRIMARY_FONT_NAME)
        );
        assert!(
            proportional_fonts
                .iter()
                .any(|font| font == UI_THAI_FONT_NAME),
            "thai fallback should remain available for proportional text"
        );
        assert!(
            proportional_fonts.len() > 2,
            "default proportional fallback fonts should remain available"
        );

        let monospace_fonts = fonts
            .families
            .get(&egui::FontFamily::Monospace)
            .expect("monospace family should exist");
        assert!(
            monospace_fonts.iter().any(|font| font == UI_THAI_FONT_NAME),
            "thai fallback should remain available for monospace text"
        );
        assert!(
            monospace_fonts.len() > 1,
            "default monospace fallback fonts should remain available"
        );
    }
}
