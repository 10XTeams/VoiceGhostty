//! 界面组件:图标按钮、分段切换、键帽、主次按钮、设置行、浮窗标题栏。
//! 全部按 `style` 的规范来画,egui 的默认控件只用在输入框、下拉框、复选框这类里层。

use egui::{pos2, vec2, Align2, Color32, CornerRadius, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

use crate::icons::{self, Icon};
use crate::style::{self, alpha, palette};

/// 无边框的图标按钮(悬停才出底色)。`on` 为 true 时保持按下态(对应的浮窗开着)
pub fn icon_button(ui: &mut Ui, icon: Icon, on: bool, tip: impl Into<String>) -> Response {
    icon_button_sized(ui, icon, on, 28.0, 15.0).on_hover_text(tip.into())
}

pub fn icon_button_sized(ui: &mut Ui, icon: Icon, on: bool, side: f32, glyph: f32) -> Response {
    let p = palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        Some(p.pressed)
    } else if resp.hovered() || on {
        Some(p.hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, style::RADIUS, fill);
    }
    let color = if resp.hovered() || on { p.text } else { p.text2 };
    icons::paint(ui.painter(), rect, glyph, icon, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 分段切换(像 macOS 的 NSSegmentedControl)。返回被点中的下标
pub fn segmented(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
    let p = palette(ui.ctx());
    let font = crate::fonts::medium(12.0);
    let pad = vec2(11.0, 0.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|o| ui.fonts_mut(|f| f.layout_no_wrap(o.to_string(), font.clone(), Color32::WHITE).size().x) + pad.x * 2.0)
        .collect();
    let h = 24.0;
    let total = widths.iter().sum::<f32>() + 4.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    ui.painter().rect_filled(outer, 8.0, p.control);
    let mut x = outer.min.x + 2.0;
    let mut clicked = None;
    for (i, (label, w)) in options.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(pos2(x, outer.min.y + 2.0), vec2(*w, h - 4.0));
        let resp = ui.interact(rect, ui.id().with(("seg", label)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = i == selected;
        if on {
            ui.painter().rect_filled(rect.translate(vec2(0.0, 0.5)), 6.0, Color32::from_black_alpha(if p.dark { 60 } else { 18 }));
            ui.painter().rect_filled(rect, 6.0, if p.dark { p.pressed } else { p.term });
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, alpha(p.hover, 0.6));
        }
        let color = if on { p.text } else if resp.hovered() { p.text } else { p.text2 };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, *label, font.clone(), color);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += w;
    }
    clicked
}

/// 把快捷键文字拆成一个个键:"⌘⇧M" → ⌘ ⇧ M;"Ctrl+Shift+M" → Ctrl Shift M
pub fn split_keys(label: &str) -> Vec<String> {
    // 范围("⌘1…⌘8")拆开反而看不懂,整体一个键帽
    if label.contains('…') {
        return vec![label.to_string()];
    }
    // Linux / Windows 的写法:修饰键是单词,用 + 连接;"Ctrl+Shift++" 的最后一个 + 是键本身
    if ["Ctrl", "Shift", "Alt", "Super"].iter().any(|m| label.contains(m)) {
        let (body, plus) = match label.strip_suffix("++") {
            Some(b) => (b, true),
            None => (label, false),
        };
        let mut keys: Vec<String> = body.split('+').filter(|s| !s.is_empty()).map(String::from).collect();
        if plus {
            keys.push("+".into());
        }
        return keys;
    }
    let mut keys = vec![];
    let mut rest = String::new();
    for ch in label.chars() {
        if "⌘⇧⌥⌃".contains(ch) {
            keys.push(ch.to_string());
        } else {
            rest.push(ch);
        }
    }
    if !rest.is_empty() {
        keys.push(rest);
    }
    keys
}

/// 一个键帽
pub fn keycap(ui: &mut Ui, key: &str) -> Response {
    let p = palette(ui.ctx());
    let font = crate::fonts::medium(11.0);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(key.to_string(), font, p.text2));
    let size = vec2((galley.size().x + 10.0).max(20.0), 19.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect.translate(vec2(0.0, 1.0)), 5.0, p.border);
    painter.rect_filled(rect, 5.0, p.control);
    painter.galley(rect.center() - galley.size() / 2.0, galley, p.text2);
    resp
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Danger,
}

pub fn button(ui: &mut Ui, text: &str, kind: Kind, enabled: bool) -> Response {
    let p = palette(ui.ctx());
    let font = crate::fonts::medium(12.5);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, Color32::WHITE));
    let size = vec2((galley.size().x + 28.0).max(72.0), 28.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let down = enabled && resp.is_pointer_button_down_on();
    let hover = enabled && resp.hovered();
    let (fill, fg) = match kind {
        Kind::Primary => (p.accent, if p.dark { Color32::from_rgb(8, 22, 34) } else { Color32::WHITE }),
        Kind::Danger => (p.error, Color32::WHITE),
        Kind::Secondary => (p.control, p.text),
    };
    let fill = if down {
        style::mix(fill, Color32::BLACK, 0.12)
    } else if hover {
        if kind == Kind::Secondary { p.hover } else { style::mix(fill, Color32::WHITE, 0.10) }
    } else {
        fill
    };
    let (fill, fg) = if enabled { (fill, fg) } else { (alpha(fill, 0.45), alpha(fg, 0.5)) };
    ui.painter().rect_filled(rect, style::RADIUS, fill);
    if kind == Kind::Secondary {
        ui.painter().rect_stroke(rect, style::RADIUS, Stroke::new(1.0, alpha(p.border, 0.8)), egui::StrokeKind::Inside);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// 设置分组的小标题
pub fn section(ui: &mut Ui, title: &str) {
    let p = palette(ui.ctx());
    ui.add_space(4.0);
    ui.label(RichText::new(title).font(crate::fonts::semibold(11.5)).color(p.text3));
    ui.add_space(2.0);
}

/// 一行设置:左边标题(下面可带一行说明),右边控件。宽度占满
pub fn setting_row(ui: &mut Ui, label: &str, hint: Option<&str>, control: impl FnOnce(&mut Ui)) {
    let p = palette(ui.ctx());
    let width = ui.available_width();
    ui.allocate_ui_with_layout(vec2(width, 0.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(label).color(p.text));
            if let Some(h) = hint {
                ui.label(RichText::new(h).font(style::small()).color(p.text3));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// 一组设置的卡片底
pub fn card<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    let p = palette(ui.ctx());
    egui::Frame::NONE
        .fill(if p.dark { style::mix(p.surface, Color32::WHITE, 0.025) } else { p.chrome })
        .stroke(Stroke::new(1.0, alpha(p.border, 0.7)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.set_width(ui.available_width());
            content(ui)
        })
        .inner
}

/// 卡片里行与行之间的细线
pub fn hairline(ui: &mut Ui) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, alpha(p.border, 0.7)));
}

/// 浮窗:没有 egui 默认标题栏(折叠三角、居中标题),换成左对齐标题 + 右上角关闭
pub fn window(title: &'static str) -> egui::Window<'static> {
    egui::Window::new(title).title_bar(false).collapsible(false).resizable(false)
}

/// 浮窗标题行。返回 true 表示点了关闭
pub fn window_header(ui: &mut Ui, title: &str, subtitle: Option<&str>) -> bool {
    let p = palette(ui.ctx());
    let mut close = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).font(crate::fonts::semibold(style::TITLE)).color(p.text));
            if let Some(s) = subtitle {
                ui.label(RichText::new(s).font(style::small()).color(p.text3));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if icon_button_sized(ui, Icon::Close, false, 24.0, 13.0).on_hover_text("关闭(Esc)").clicked() {
                close = true;
            }
        });
    });
    ui.add_space(6.0);
    close || ui.input(|i| i.key_pressed(egui::Key::Escape))
}

/// 小圆点(状态灯)
pub fn dot(ui: &Ui, center: egui::Pos2, r: f32, color: Color32, glow: bool) {
    if glow {
        ui.painter().circle_filled(center, r + 2.5, alpha(color, 0.22));
    }
    ui.painter().circle_filled(center, r, color);
}

#[cfg(test)]
mod tests {
    use super::split_keys;

    #[test]
    fn splits_shortcut_labels_into_keys() {
        assert_eq!(split_keys("⌘⇧M"), ["⌘", "⇧", "M"]);
        assert_eq!(split_keys("Ctrl+Shift+M"), ["Ctrl", "Shift", "M"]);
        assert_eq!(split_keys("⌘+"), ["⌘", "+"]);
        assert_eq!(split_keys("Ctrl+Shift++"), ["Ctrl", "Shift", "+"]);
        assert_eq!(split_keys("⌘1…⌘8"), ["⌘1…⌘8"]);
    }
}
