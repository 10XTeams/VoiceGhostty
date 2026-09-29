//! 终端控件:把 alacritty 的网格画到 egui 上,并把键盘/输入法/滚轮/粘贴转成 PTY 输入。
//! 按住空格说话也在这里拦截——只有终端有焦点时空格才有这层含义。

use std::time::{Duration, Instant};

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape};
use egui::{pos2, vec2, Align2, Color32, Event, EventFilter, FontId, ImeEvent, Rect, Sense, Stroke};

use vg_core::{Rgb, Theme};

use super::keys;
use super::session::{GridSize, TerminalSession};
use crate::ptt::{PttAction, PushToTalk};

/// 每个终端窗格跨帧保留的输入状态
#[derive(Default)]
pub struct InputState {
    pub ptt: PushToTalk,
    /// 输入法正在组字(拼音还没上屏)。组字时空格属于输入法,不能拦截成按住说话
    pub preedit: String,
    scroll_accum: f32,
}

pub struct ViewOptions<'a> {
    pub theme: &'a Theme,
    pub font_size: f32,
    pub hold: Duration,
    /// 为 false 时空格就是空格(语音关闭 / 正在识别)
    pub ptt_enabled: bool,
}

/// 返回需要 App 处理的按住说话动作(开始/停止录音)
pub fn show(
    ui: &mut egui::Ui,
    session: &mut TerminalSession,
    state: &mut InputState,
    opts: &ViewOptions,
) -> Vec<PttAction> {
    let font = FontId::monospace(opts.font_size);
    let (cw, ch) = ui
        .ctx()
        .fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));

    let rect = ui.available_rect_before_wrap();
    let id = ui.id().with("terminal");
    let response = ui.interact(rect, id, Sense::click());
    ui.allocate_rect(rect, Sense::hover());

    // 点一下获取焦点;没有别的控件抢焦点时终端默认持有焦点
    if response.clicked() || ui.memory(|m| m.focused().is_none()) {
        response.request_focus();
    }
    let focused = response.has_focus();
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
        )
    });

    session.resize(GridSize {
        cols: ((rect.width() / cw).floor() as u16).max(2),
        rows: ((rect.height() / ch).floor() as u16).max(1),
        cell_w: cw.round() as u16,
        cell_h: ch.round() as u16,
    });

    let mut actions = Vec::new();
    if focused {
        handle_input(ui, session, state, opts, ch, response.hovered(), &mut actions);
        // IME 候选框跟随光标
        let cursor = session.term.lock().grid().cursor.point;
        let cursor_rect = Rect::from_min_size(
            pos2(rect.min.x + cursor.column.0 as f32 * cw, rect.min.y + cursor.line.0 as f32 * ch),
            vec2(cw, ch),
        );
        ui.ctx().output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Terminal,
                rect,
                cursor_rect,
                should_interrupt_composition: false,
            })
        });
    } else if state.ptt.cancel() == PttAction::StopRecording {
        // 失焦时放弃按键;正在录音就停止
        actions.push(PttAction::StopRecording);
    }

    draw(ui, session, state, opts, rect, &font, cw, ch, focused);
    actions
}

#[allow(clippy::too_many_arguments)]
fn handle_input(
    ui: &mut egui::Ui,
    session: &mut TerminalSession,
    state: &mut InputState,
    opts: &ViewOptions,
    ch: f32,
    hovered: bool,
    actions: &mut Vec<PttAction>,
) {
    let now = Instant::now();
    let mode = *session.term.lock().mode();
    let events = ui.input(|i| i.events.clone());

    for event in events {
        match event {
            Event::Key { key: egui::Key::Space, pressed, repeat, modifiers, .. }
                if opts.ptt_enabled
                    && state.preedit.is_empty()
                    && !modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.mac_cmd =>
            {
                if pressed && !repeat {
                    state.ptt.key_down(now);
                } else if !pressed {
                    match state.ptt.key_up(now, opts.hold) {
                        PttAction::TypeSpace => session.write(&b" "[..]),
                        PttAction::None => {}
                        other => actions.push(other),
                    }
                }
            }
            // 空格由上面的按键事件负责(点按时补发),这里丢掉以免重复
            Event::Text(t) if opts.ptt_enabled && t == " " && state.preedit.is_empty() => {}
            Event::Text(t) => {
                session.term.lock().scroll_display(Scroll::Bottom);
                session.write(t.into_bytes());
            }
            Event::Key { key, pressed: true, modifiers, .. } => {
                if let Some(bytes) = keys::encode(key, modifiers, mode.contains(TermMode::APP_CURSOR)) {
                    session.term.lock().scroll_display(Scroll::Bottom);
                    session.write(bytes);
                }
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => state.preedit = text,
            Event::Ime(ImeEvent::Commit(text)) => {
                state.preedit.clear();
                session.write(text.into_bytes());
            }
            Event::Paste(text) => paste(session, mode, &text),
            // Linux/Windows 上 egui 把 Ctrl+C / Ctrl+X 变成复制/剪切事件;终端里它们应该是控制字符
            Event::Copy if !cfg!(target_os = "macos") => session.write(&b"\x03"[..]),
            Event::Cut if !cfg!(target_os = "macos") => session.write(&b"\x18"[..]),
            Event::MouseWheel { delta, unit, .. } if hovered => {
                let lines = match unit {
                    egui::MouseWheelUnit::Point => delta.y / ch,
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Page => delta.y * 20.0,
                };
                state.scroll_accum += lines;
                let whole = state.scroll_accum.trunc() as i32;
                if whole != 0 {
                    state.scroll_accum -= whole as f32;
                    scroll(session, mode, whole);
                }
            }
            _ => {}
        }
    }

    match state.ptt.tick(now, opts.hold) {
        PttAction::None => {}
        a => actions.push(a),
    }
    if state.ptt.is_pending() {
        ui.ctx().request_repaint_after(Duration::from_millis(30));
    }
}

fn paste(session: &TerminalSession, mode: TermMode, text: &str) {
    session.term.lock().scroll_display(Scroll::Bottom);
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // 去掉内容里的结束标记,防止粘贴内容"逃出"括号模式直接执行
        let clean = text.replace("\x1b[201~", "");
        session.write(format!("\x1b[200~{clean}\x1b[201~").into_bytes());
    } else {
        session.write(text.replace("\r\n", "\r").replace('\n', "\r").into_bytes());
    }
}

fn scroll(session: &TerminalSession, mode: TermMode, lines: i32) {
    // 全屏程序(less/vim)里滚轮转成方向键
    if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && !mode.intersects(TermMode::MOUSE_MODE) {
        let seq: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        session.write(seq.repeat(lines.unsigned_abs() as usize));
    } else {
        session.term.lock().scroll_display(Scroll::Delta(lines));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &egui::Ui,
    session: &TerminalSession,
    state: &InputState,
    opts: &ViewOptions,
    rect: Rect,
    font: &FontId,
    cw: f32,
    ch: f32,
    focused: bool,
) {
    let theme = opts.theme;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, c32(theme.background));

    let term = session.term.lock();
    let content = term.renderable_content();
    let offset = content.display_offset as i32;
    let colors = content.colors;

    for indexed in content.display_iter {
        let cell = &indexed.cell;
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let line = indexed.point.line.0 + offset;
        let x = rect.min.x + indexed.point.column.0 as f32 * cw;
        let y = rect.min.y + line as f32 * ch;

        let mut fg = resolve(&cell.fg, colors, theme);
        let mut bg = resolve(&cell.bg, colors, theme);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::DIM) {
            fg = dim(fg);
        }
        let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2.0 * cw } else { cw };
        let cell_rect = Rect::from_min_size(pos2(x, y), vec2(width, ch));

        if bg != theme.background {
            painter.rect_filled(cell_rect, 0.0, c32(bg));
        }
        if cell.c != ' ' && cell.c != '\t' && !cell.flags.contains(Flags::HIDDEN) {
            painter.text(pos2(x, y), Align2::LEFT_TOP, cell.c, font.clone(), c32(fg));
        }
        if cell.flags.intersects(Flags::ALL_UNDERLINES) {
            let uy = y + ch - 1.0;
            painter.line_segment([pos2(x, uy), pos2(x + width, uy)], Stroke::new(1.0, c32(fg)));
        }
        if cell.flags.contains(Flags::STRIKEOUT) {
            let sy = y + ch / 2.0;
            painter.line_segment([pos2(x, sy), pos2(x + width, sy)], Stroke::new(1.0, c32(fg)));
        }
    }

    // 光标
    let cursor = content.cursor;
    let cy = cursor.point.line.0 + offset;
    if cursor.shape != CursorShape::Hidden && cy >= 0 {
        let pos = pos2(rect.min.x + cursor.point.column.0 as f32 * cw, rect.min.y + cy as f32 * ch);
        let color = c32(theme.cursor);
        let block = Rect::from_min_size(pos, vec2(cw, ch));
        let _ = match (focused, cursor.shape) {
            (true, CursorShape::Beam) => {
                painter.rect_filled(Rect::from_min_size(pos, vec2(2.0, ch)), 0.0, color)
            }
            (true, CursorShape::Underline) => painter.rect_filled(
                Rect::from_min_size(pos2(pos.x, pos.y + ch - 2.0), vec2(cw, 2.0)),
                0.0,
                color,
            ),
            (true, _) => painter.rect_filled(block, 0.0, color.gamma_multiply(0.6)),
            (false, _) => {
                painter.rect_stroke(block, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside)
            }
        };

        // 输入法组字中的拼音,画在光标处并加下划线
        if !state.preedit.is_empty() {
            let galley = painter.layout_no_wrap(state.preedit.clone(), font.clone(), c32(theme.foreground));
            let r = Rect::from_min_size(pos, galley.size());
            painter.rect_filled(r, 0.0, c32(theme.background));
            painter.galley(pos, galley, c32(theme.foreground));
            painter.line_segment(
                [pos2(r.min.x, r.max.y - 1.0), pos2(r.max.x, r.max.y - 1.0)],
                Stroke::new(1.0, c32(theme.foreground)),
            );
        }
    }
}

fn c32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn dim(c: Rgb) -> Rgb {
    Rgb((c.0 as f32 * 0.66) as u8, (c.1 as f32 * 0.66) as u8, (c.2 as f32 * 0.66) as u8)
}

fn resolve(color: &AnsiColor, colors: &Colors, theme: &Theme) -> Rgb {
    let index = match color {
        AnsiColor::Spec(rgb) => return Rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Named(n) => *n as usize,
        AnsiColor::Indexed(i) => *i as usize,
    };
    match colors[index] {
        Some(rgb) => Rgb(rgb.r, rgb.g, rgb.b),
        None => resolve_index(theme, index),
    }
}

/// 颜色下标(与 alacritty 的 NamedColor 编号一致)→ 主题颜色
pub fn resolve_index(theme: &Theme, index: usize) -> Rgb {
    match index {
        0..=15 => theme.ansi[index],
        // 6×6×6 色立方
        16..=231 => {
            let i = index - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            Rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        // 24 级灰阶
        232..=255 => {
            let v = (8 + (index - 232) * 10) as u8;
            Rgb(v, v, v)
        }
        256 | 267 => theme.foreground,
        257 => theme.background,
        258 => theme.cursor,
        259..=266 => dim(theme.ansi[index - 259]),
        268 => dim(theme.foreground),
        _ => theme.foreground,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_palette() {
        let t = Theme::DARK;
        assert_eq!(resolve_index(&t, 1), t.ansi[1]);
        assert_eq!(resolve_index(&t, 16), Rgb(0, 0, 0));
        assert_eq!(resolve_index(&t, 231), Rgb(255, 255, 255));
        assert_eq!(resolve_index(&t, 232), Rgb(8, 8, 8));
        assert_eq!(resolve_index(&t, 257), t.background);
    }
}
