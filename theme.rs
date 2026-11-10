
//! Glassmorphism look for egui 0.31 with Night, Day and Sepia reading themes.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Margin, Pos2, Shadow, Stroke, TextStyle,
};

/// Translucent white. Premultiplied: white at alpha `a` is (a, a, a, a).
pub const fn white(alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha)
}

/// The three built-in looks. Sepia is a warm "paper mushaf" light theme.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Dark,
    Light,
    Sepia,
}

impl Theme {
    pub fn dark(self) -> bool {
        matches!(self, Theme::Dark)
    }
    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Night",
            Theme::Light => "Day",
            Theme::Sepia => "Sepia",
        }
    }
    pub fn next(self) -> Theme {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Sepia,
            Theme::Sepia => Theme::Dark,
        }
    }
    pub fn to_u8(self) -> u8 {
        match self {
            Theme::Dark => 0,
            Theme::Light => 1,
            Theme::Sepia => 2,
        }
    }
    pub fn from_u8(v: u8) -> Theme {
        match v {
            1 => Theme::Light,
            2 => Theme::Sepia,
            _ => Theme::Dark,
        }
    }
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

pub fn arabic_text(t: Theme) -> Color32 {
    match t {
        Theme::Dark => Color32::from_rgb(244, 246, 240),
        Theme::Light => Color32::from_rgb(22, 26, 24),
        Theme::Sepia => Color32::from_rgb(62, 44, 28),
    }
}

pub fn translation_text(t: Theme) -> Color32 {
    match t {
        Theme::Dark => Color32::from_rgb(188, 202, 196),
        Theme::Light => Color32::from_rgb(58, 70, 66),
        Theme::Sepia => Color32::from_rgb(110, 88, 62),
    }
}

/// Colour of Arabic text for verses the reader has tapped.
pub fn tapped_text(t: Theme) -> Color32 {
    match t {
        Theme::Dark => Color32::from_rgb(232, 190, 110),
        Theme::Light => Color32::from_rgb(176, 104, 18),
        Theme::Sepia => Color32::from_rgb(160, 84, 24),
    }
}

/// Colour of the Arabic verse that is being recited.
pub fn playing_text(t: Theme) -> Color32 {
    match t {
        Theme::Dark => Color32::from_rgb(110, 225, 160),
        Theme::Light => Color32::from_rgb(0, 128, 72),
        Theme::Sepia => Color32::from_rgb(20, 112, 62),
    }
}

pub fn accent(t: Theme) -> Color32 {
    if t.dark() {
        Color32::from_rgb(120, 220, 190)
    } else {
        Color32::from_rgb(10, 120, 90)
    }
}

pub fn glass(t: Theme, alpha: u8) -> Color32 {
    if t.dark() {
        white(alpha)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, alpha.saturating_mul(3))
    }
}

pub fn border(t: Theme, strength: u8) -> Color32 {
    if t.dark() {
        white(strength)
    } else {
        Color32::from_black_alpha(strength / 2)
    }
}

/// Fill of the reading page (almost opaque so words stay crisp).
pub fn page_fill(t: Theme) -> Color32 {
    match t {
        Theme::Dark => Color32::from_rgba_unmultiplied(10, 16, 16, 240),
        Theme::Light => Color32::from_rgba_unmultiplied(252, 249, 240, 246),
        Theme::Sepia => Color32::from_rgba_unmultiplied(247, 238, 214, 250),
    }
}

// ---------------------------------------------------------------------------
// Global visuals
// ---------------------------------------------------------------------------

pub fn apply(ctx: &egui::Context, t: Theme) {
    // Never follow the phone's light/dark setting. Otherwise egui swaps to its own
    // default style when the phone's mode differs from the app's theme (a Night app
    // on a phone in light mode), and the text becomes unreadable.
    ctx.set_theme(if t.dark() {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });

    let mut v = if t.dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = match t {
        Theme::Dark => Color32::from_rgba_unmultiplied(20, 28, 30, 235),
        Theme::Light => Color32::from_rgba_unmultiplied(252, 249, 240, 246),
        Theme::Sepia => Color32::from_rgba_unmultiplied(250, 242, 220, 246),
    };
    v.window_stroke = Stroke::new(1.0, border(t, 90));
    v.extreme_bg_color = if t.dark() {
        Color32::from_rgba_unmultiplied(0, 0, 0, 80)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 170)
    };
    v.selection.bg_fill = if t.dark() {
        Color32::from_rgb(40, 130, 110)
    } else {
        Color32::from_rgb(120, 200, 175)
    };
    if t.dark() {
        v.widgets.inactive.bg_fill = white(28);
        v.widgets.inactive.weak_bg_fill = white(28);
        v.widgets.hovered.bg_fill = white(50);
        v.widgets.hovered.weak_bg_fill = white(50);
        v.widgets.active.bg_fill = white(72);
        v.widgets.active.weak_bg_fill = white(72);
    } else {
        let boost = matches!(t, Theme::Sepia);
        let a = |v: u8| if boost { v.saturating_add(8) } else { v };
        v.widgets.inactive.bg_fill = Color32::from_black_alpha(a(14));
        v.widgets.inactive.weak_bg_fill = Color32::from_black_alpha(a(14));
        v.widgets.hovered.bg_fill = Color32::from_black_alpha(a(28));
        v.widgets.hovered.weak_bg_fill = Color32::from_black_alpha(a(28));
        v.widgets.active.bg_fill = Color32::from_black_alpha(a(44));
        v.widgets.active.weak_bg_fill = Color32::from_black_alpha(a(44));
    }
    // Fully rounded (pill / circle) buttons everywhere. The top bar switches
    // back with `square_buttons`.
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(255);
    }
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 8.0);
        s.spacing.interact_size.y = 38.0;
        s.interaction.selectable_labels = false;
        s.text_styles
            .insert(TextStyle::Body, egui::FontId::proportional(17.0));
        s.text_styles
            .insert(TextStyle::Button, egui::FontId::proportional(17.0));
        s.text_styles
            .insert(TextStyle::Heading, egui::FontId::proportional(26.0));
        // Small text keeps egui's built-in font; the family is set up in `setup_fonts`.
        s.text_styles.insert(
            TextStyle::Small,
            egui::FontId::new(13.0, egui::FontFamily::Name("small_text".into())),
        );
    });
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

fn soft_shadow(t: Theme) -> Shadow {
    Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark() { 70 } else { 28 }),
    }
}

pub fn card_frame(t: Theme, tinted: bool) -> Frame {
    let fill = if tinted {
        let a = accent(t);
        Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 36)
    } else {
        glass(t, 24)
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border(t, if tinted { 120 } else { 64 })))
        .corner_radius(CornerRadius::same(22))
        .inner_margin(Margin::same(14))
        .shadow(soft_shadow(t))
}

pub fn glass_card<R>(
    ui: &mut egui::Ui,
    tinted: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let t = if ui.visuals().dark_mode { Theme::Dark } else { Theme::Light };
    card_frame(t, tinted).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add_contents(ui)
    })
}

pub fn page_frame(t: Theme) -> Frame {
    Frame::new()
        .fill(page_fill(t))
        .inner_margin(Margin::symmetric(0, 8))
}

pub fn inset_frame(t: Theme) -> Frame {
    Frame::new().fill(page_fill(t))
}

pub fn bar_frame(t: Theme) -> Frame {
    let fill = match t {
        Theme::Dark => Color32::from_rgba_unmultiplied(16, 26, 28, 240),
        Theme::Light => Color32::from_rgba_unmultiplied(244, 242, 234, 244),
        Theme::Sepia => Color32::from_rgba_unmultiplied(240, 229, 201, 246),
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border(t, 40)))
        .inner_margin(Margin::symmetric(12, 8))
}

pub fn sidebar_frame(t: Theme) -> Frame {
    let fill = match t {
        Theme::Dark => Color32::from_rgba_unmultiplied(14, 22, 24, 226),
        Theme::Light => Color32::from_rgba_unmultiplied(246, 244, 236, 236),
        Theme::Sepia => Color32::from_rgba_unmultiplied(242, 232, 206, 238),
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border(t, 40)))
        .inner_margin(Margin::same(10))
}

pub fn window_frame(t: Theme) -> Frame {
    let fill = match t {
        Theme::Dark => Color32::from_rgba_unmultiplied(20, 28, 30, 238),
        Theme::Light => Color32::from_rgba_unmultiplied(252, 249, 240, 246),
        Theme::Sepia => Color32::from_rgba_unmultiplied(250, 242, 220, 248),
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border(t, 100)))
        .corner_radius(CornerRadius::same(26))
        .inner_margin(Margin::same(16))
        .shadow(Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(if t.dark() { 120 } else { 60 }),
        })
}

// ---------------------------------------------------------------------------
// Backdrop
// ---------------------------------------------------------------------------

pub fn paint_backdrop(ctx: &egui::Context, t: Theme) {
    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());

    let (top, bottom) = match t {
        Theme::Dark => (Color32::from_rgb(12, 20, 24), Color32::from_rgb(6, 36, 42)),
        Theme::Light => (Color32::from_rgb(240, 246, 238), Color32::from_rgb(212, 232, 226)),
        Theme::Sepia => (Color32::from_rgb(238, 226, 196), Color32::from_rgb(222, 199, 152)),
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));

    let (w, h) = (rect.width(), rect.height());
    let origin = rect.left_top();
    let (c1, c2, c3, alpha) = match t {
        Theme::Dark => (
            Color32::from_rgb(255, 80, 160),
            Color32::from_rgb(90, 120, 255),
            Color32::from_rgb(40, 220, 200),
            3,
        ),
        Theme::Light => (
            Color32::from_rgb(255, 170, 200),
            Color32::from_rgb(150, 180, 255),
            Color32::from_rgb(120, 225, 200),
            4,
        ),
        Theme::Sepia => (
            Color32::from_rgb(214, 164, 110),
            Color32::from_rgb(190, 150, 120),
            Color32::from_rgb(160, 170, 120),
            4,
        ),
    };
    glow(&painter, origin + egui::vec2(w * 0.15, h * 0.18), w * 0.65, c1, alpha);
    glow(&painter, origin + egui::vec2(w * 0.92, h * 0.50), w * 0.60, c2, alpha);
    glow(&painter, origin + egui::vec2(w * 0.25, h * 0.90), w * 0.70, c3, alpha);
}

fn glow(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32, alpha: u8) {
    const STEPS: u32 = 14;
    for k in 0..STEPS {
        let t = k as f32 / STEPS as f32;
        painter.circle_filled(
            center,
            radius * (1.0 - t),
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
        );
    }
}

/// egui's default (square-ish) widget corners. Used for the top bar, which
/// keeps its original look while buttons elsewhere are round.
pub fn square_buttons(v: &mut egui::Visuals) {
    v.widgets.inactive.corner_radius = CornerRadius::same(2);
    v.widgets.hovered.corner_radius = CornerRadius::same(3);
    v.widgets.active.corner_radius = CornerRadius::same(2);
    v.widgets.open.corner_radius = CornerRadius::same(2);
}
