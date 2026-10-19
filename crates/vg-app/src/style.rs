//! 设计规范:颜色、字号、间距、圆角都从这里来。颜色全部从当前终端主题推导,
//! 换主题(Dark / Light / Solarized / Dracula)时整套界面跟着换,不会出现"终端是紫的、外框是灰的"。
//!
//! 层次(深色主题下从暗到亮):chrome(标签栏、状态栏)< 终端底色 < surface(浮窗)< hover。
//! 终端是主角,所以外框比它暗半档,当前标签页与终端同色,看上去是从终端里"长"出来的。

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2};

use vg_core::{Rgb, Theme};

/// 一套主题推导出的全部颜色。存在 egui Context 里,任何控件用 `palette(ctx)` 取
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// 终端底色
    pub term: Color32,
    /// 标签栏、状态栏
    pub chrome: Color32,
    /// 浮窗、菜单、输入框外的卡片
    pub surface: Color32,
    /// 按钮常态
    pub control: Color32,
    /// 悬停
    pub hover: Color32,
    /// 按下 / 选中的分段
    pub pressed: Color32,
    /// 细分隔线、浮窗描边
    pub border: Color32,
    /// 输入框底
    pub field: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub accent: Color32,
    /// 录音
    pub rec: Color32,
    pub busy: Color32,
    pub done: Color32,
    pub warn: Color32,
    pub error: Color32,
}

fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// a → b 插值 t(0 = a,1 = b)
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// 不透明色加透明度(egui 用预乘 alpha,不能直接改 a 通道)
pub fn alpha(color: Color32, a: f32) -> Color32 {
    color.gamma_multiply(a)
}

impl Palette {
    pub fn of(theme: &Theme) -> Palette {
        let term = c(theme.background);
        let fg = c(theme.foreground);
        let lum = 0.299 * term.r() as f32 + 0.587 * term.g() as f32 + 0.114 * term.b() as f32;
        let dark = lum < 128.0;
        let (black, white) = (Color32::BLACK, Color32::WHITE);
        if dark {
            Palette {
                dark,
                term,
                chrome: mix(term, black, 0.22),
                surface: mix(term, white, 0.045),
                control: mix(term, white, 0.075),
                hover: mix(term, white, 0.11),
                pressed: mix(term, white, 0.15),
                border: mix(term, white, 0.10),
                field: mix(term, black, 0.30),
                text: mix(fg, white, 0.25),
                text2: mix(fg, term, 0.36),
                text3: mix(fg, term, 0.58),
                accent: Color32::from_rgb(0x5C, 0xC8, 0xFF),
                rec: Color32::from_rgb(0xFF, 0x5A, 0x52),
                busy: Color32::from_rgb(0xF2, 0xB5, 0x44),
                done: Color32::from_rgb(0x3C, 0xCB, 0x7F),
                warn: Color32::from_rgb(0xF0, 0xA2, 0x3B),
                error: Color32::from_rgb(0xFF, 0x6B, 0x6B),
            }
        } else {
            Palette {
                dark,
                term,
                chrome: mix(term, black, 0.045),
                surface: term,
                control: mix(term, black, 0.055),
                hover: mix(term, black, 0.085),
                pressed: mix(term, black, 0.12),
                border: mix(term, black, 0.12),
                field: mix(term, black, 0.03),
                text: mix(fg, black, 0.2),
                text2: mix(fg, term, 0.40),
                text3: mix(fg, term, 0.60),
                accent: Color32::from_rgb(0x0A, 0x76, 0xCC),
                rec: Color32::from_rgb(0xE5, 0x3B, 0x33),
                busy: Color32::from_rgb(0xC7, 0x86, 0x10),
                done: Color32::from_rgb(0x1F, 0x9D, 0x5A),
                warn: Color32::from_rgb(0xC2, 0x6D, 0x0A),
                error: Color32::from_rgb(0xD1, 0x34, 0x34),
            }
        }
    }
}

pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|d| d.get_temp::<Palette>(egui::Id::NULL)).unwrap_or_else(|| Palette::of(&Theme::DARK))
}

// MARK: 尺寸

/// 标签栏、状态栏的高度
pub const BAR_H: f32 = 40.0;
pub const STATUS_H: f32 = 34.0;
/// 终端文字离窗格边缘的距离
pub const TERM_PAD: Vec2 = Vec2::new(10.0, 6.0);
pub const RADIUS: u8 = 7;
pub const RADIUS_LG: u8 = 12;

pub const BODY: f32 = 13.0;
pub const SMALL: f32 = 11.5;
pub const TITLE: f32 = 15.0;
/// 标签页标题,与窗口标题同字号;标签本体高 28,与 macOS 原生标题栏一致
pub const TAB: f32 = 15.0;

pub fn body() -> FontId {
    FontId::proportional(BODY)
}

pub fn small() -> FontId {
    FontId::proportional(SMALL)
}

pub fn apply(ctx: &egui::Context, theme: &Theme) {
    let p = Palette::of(theme);
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, p));

    let mut visuals = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    visuals.panel_fill = p.chrome;
    visuals.window_fill = p.surface;
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_LG);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_shadow = Shadow { offset: [0, 12], blur: 36, spread: 0, color: Color32::from_black_alpha(if p.dark { 120 } else { 40 }) };
    visuals.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 28 }) };
    visuals.extreme_bg_color = p.field;
    visuals.text_edit_bg_color = Some(p.field);
    visuals.faint_bg_color = p.control;
    visuals.code_bg_color = p.control;
    visuals.hyperlink_color = p.accent;
    visuals.warn_fg_color = p.warn;
    visuals.error_fg_color = p.error;
    visuals.selection.bg_fill = alpha(p.accent, 0.32);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.text_cursor.stroke = Stroke::new(1.5, p.accent);
    visuals.window_highlight_topmost = false;
    visuals.striped = false;
    visuals.indent_has_left_vline = false;

    let radius = CornerRadius::same(RADIUS);
    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text2);
    w.noninteractive.corner_radius = radius;
    for (state, fill, fg) in [
        (&mut w.inactive, p.control, p.text),
        (&mut w.hovered, p.hover, p.text),
        (&mut w.active, p.pressed, p.text),
        (&mut w.open, p.hover, p.text),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.5, fg);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    // 复选框、单选框的框线要看得见
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);

    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(TITLE, FontFamily::Name(crate::fonts::SEMIBOLD.into()))),
            (TextStyle::Body, body()),
            (TextStyle::Button, body()),
            (TextStyle::Small, small()),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(10.0, 4.0);
        s.interact_size = Vec2::new(28.0, 26.0);
        s.window_margin = Margin::same(18);
        s.menu_margin = Margin::same(6);
        s.icon_width = 15.0;
        s.icon_width_inner = 9.0;
        s.icon_spacing = 8.0;
        s.combo_width = 180.0;
        s.tooltip_width = 360.0;
        style.interaction.tooltip_delay = 0.35;
    });
}
