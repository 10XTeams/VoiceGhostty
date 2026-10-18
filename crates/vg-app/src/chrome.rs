//! 主窗口外框:标签栏、浮动搜索框、状态栏(语音胶囊)、快捷键与退出确认浮窗。
//! 只管画和收集点击;真正的动作仍由 app.rs 里的方法完成。

use std::time::Duration;

use egui::{pos2, vec2, Align2, Color32, CornerRadius, Key, Rect, RichText, Sense, Stroke, Ui};

use vg_core::{SessionStatus, VoiceEngineKind};

use super::{VoiceGhosttyApp, VoiceUi};
use crate::icons::{self, Icon};
use crate::kit;
use crate::pipeline::VoiceMode;
use crate::ptt::PttAction;
use crate::shortcuts::{self, Action};
use crate::style::{self, alpha, mix, palette, Palette};
use crate::terminal::search::SearchResult;
use crate::widgets;

/// 状态文字显示多久;最后一秒淡出。错误一直留到下一条
const STATUS_TTL: f32 = 15.0;
const STATUS_FADE: f32 = 1.2;
/// 录音波形的柱子数
pub(crate) const WAVE_BARS: usize = 28;

impl VoiceGhosttyApp {
    // MARK: 标签栏

    pub(crate) fn tab_bar(&mut self, ui: &mut Ui) {
        let p = palette(ui.ctx());
        let full = ui.max_rect();
        let tab_h = full.height() - 6.0;
        let mut active_span = None;
        let mut select = None;
        let mut close = None;
        let mut rename_start = None;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for i in 0..self.ws.tabs.len() {
                let active = i == self.ws.active_tab;
                let statuses: Vec<SessionStatus> = self.ws.tabs[i]
                    .panes()
                    .iter()
                    .filter_map(|id| self.ws.panes.get(id))
                    .map(|p| p.session.shared.status())
                    .collect();
                let lit: Vec<SessionStatus> = statuses.iter().copied().filter(|s| *s != SessionStatus::Normal).collect();
                let title = widgets::truncate(&self.tab_title(i), 26);

                let title_font = crate::fonts::medium(style::TAB);
                let title_w = ui.fonts_mut(|f| f.layout_no_wrap(title.clone(), title_font.clone(), Color32::WHITE).size().x);
                let dots_w = if lit.is_empty() { 0.0 } else { lit.len().min(4) as f32 * 9.0 + 4.0 };
                let width = (14.0 + 14.0 + dots_w + title_w + 26.0).clamp(120.0, 240.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(width, tab_h), Sense::click());
                let rect = rect.translate(vec2(0.0, full.bottom() - rect.bottom()));
                let hovered = resp.hovered();

                if active {
                    let r = CornerRadius { nw: 9, ne: 9, sw: 0, se: 0 };
                    ui.painter().rect_filled(rect, r, p.term);
                    active_span = Some(rect.x_range());
                } else if hovered {
                    ui.painter().rect_filled(rect.shrink2(vec2(0.0, 4.0)).translate(vec2(0.0, -2.0)), 7.0, alpha(p.hover, 0.55));
                }

                // 序号 · 状态灯 · 标题 · 关闭
                let mut x = rect.min.x + 12.0;
                let cy = rect.center().y - 1.0;
                let num_color = if active { p.text3 } else { alpha(p.text3, 0.8) };
                ui.painter().text(pos2(x, cy), Align2::LEFT_CENTER, format!("{}", i + 1), crate::fonts::medium(11.0), num_color);
                x += 14.0;
                for s in lit.iter().take(4) {
                    let color = widgets::status_color_of(&p, *s);
                    let pulse = *s == SessionStatus::Busy;
                    let a = if pulse { 0.55 + 0.45 * (ui.input(|i| i.time) as f32 * 3.0).sin().abs() } else { 1.0 };
                    kit::dot(ui, pos2(x + 3.5, cy), 3.5, alpha(color, a), *s == SessionStatus::Done);
                    x += 9.0;
                }
                if !lit.is_empty() {
                    x += 4.0;
                    if lit.contains(&SessionStatus::Busy) {
                        ui.ctx().request_repaint_after(Duration::from_millis(60));
                    }
                }

                let renaming = self.renaming.as_ref().is_some_and(|(ri, _)| *ri == i);
                if renaming {
                    let (_, draft) = self.renaming.as_mut().unwrap();
                    let edit_rect = Rect::from_min_max(pos2(x - 2.0, cy - 11.0), pos2(rect.max.x - 8.0, cy + 11.0));
                    let edit = ui.put(
                        edit_rect,
                        egui::TextEdit::singleline(draft).font(title_font.clone()).margin(vec2(4.0, 2.0)),
                    );
                    edit.request_focus();
                    let (enter, esc) = ui.input(|inp| (inp.key_pressed(Key::Enter), inp.key_pressed(Key::Escape)));
                    if esc {
                        self.renaming = None;
                    } else if enter || edit.lost_focus() {
                        let title = draft.trim().to_string();
                        self.ws.tabs[i].custom_title = (!title.is_empty()).then_some(title);
                        self.renaming = None;
                    }
                } else {
                    let color = if active { p.text } else if hovered { p.text } else { p.text2 };
                    let clip = Rect::from_min_max(pos2(x, rect.min.y), pos2(rect.max.x - 26.0, rect.max.y));
                    ui.painter().with_clip_rect(clip).text(pos2(x, cy), Align2::LEFT_CENTER, &title, title_font, color);
                }

                // 关闭按钮:当前标签页常驻,其他标签页悬停才出现
                if (active || hovered) && !renaming {
                    let close_rect = Rect::from_center_size(pos2(rect.max.x - 15.0, cy), vec2(18.0, 18.0));
                    let c = ui.interact(close_rect, ui.id().with(("tab-close", i)), Sense::click());
                    if c.hovered() {
                        ui.painter().rect_filled(close_rect, 5.0, p.hover);
                    }
                    icons::paint(ui.painter(), close_rect, 11.0, Icon::Close, if c.hovered() { p.text } else { p.text3 });
                    if c.clicked() {
                        close = Some(i);
                    }
                }

                let hover_text: Vec<String> = self.ws.tabs[i]
                    .panes()
                    .iter()
                    .filter_map(|id| self.ws.panes.get(id))
                    .map(|p| p.session.title.clone())
                    .collect();
                let resp = resp.on_hover_text(hover_text.join("\n"));
                if resp.double_clicked() {
                    rename_start = Some(i);
                } else if resp.clicked() && close.is_none() {
                    select = Some(i);
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
            }
            ui.add_space(4.0);
            // 和标签文字同一条中线
            let (slot, _) = ui.allocate_exact_size(vec2(28.0, tab_h), Sense::hover());
            let at = Rect::from_center_size(pos2(slot.center().x, full.bottom() - tab_h / 2.0 - 1.0), vec2(28.0, 28.0));
            let tip = format!("新标签页  {}", Action::NewTab.label());
            if ui.put(at, |ui: &mut Ui| kit::icon_button(ui, Icon::Plus, false, tip)).clicked() {
                self.new_tab(ui.ctx());
            }
        });

        // 底边一条细线,在当前标签页下面断开:标签页和终端连成一片
        let y = full.bottom() - 0.5;
        let line = Stroke::new(1.0, p.border);
        match active_span {
            Some(span) => {
                ui.painter().hline(full.min.x - 8.0..=span.min, y, line);
                ui.painter().hline(span.max..=full.max.x + 8.0, y, line);
            }
            None => {
                ui.painter().hline(full.x_range(), y, line);
            }
        }

        if let Some(i) = rename_start {
            self.renaming = Some((i, self.ws.tabs[i].custom_title.clone().unwrap_or_default()));
        }
        if let Some(i) = select {
            self.ws.select_tab(i);
        }
        if let Some(i) = close {
            drop(self.ws.close_tab(i));
            if self.ws.tabs.is_empty() {
                self.quit_confirmed = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    // MARK: 搜索(浮在终端右上角)

    pub(crate) fn search_box(&mut self, ctx: &egui::Context, anchor: Rect) {
        let p = palette(ctx);
        let Some(pane) = self.ws.active_pane_mut() else { return };
        let mut close = false;
        egui::Area::new(egui::Id::new("search-box"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(anchor.max.x - 16.0 - 380.0, anchor.min.y + 10.0))
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(10)
                    .shadow(ctx.global_style().visuals.popup_shadow)
                    .inner_margin(egui::Margin { left: 10, right: 4, top: 4, bottom: 4 })
                    .show(ui, |ui| {
                        ui.set_width(366.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 26.0), Sense::hover());
                            icons::paint(ui.painter(), r, 14.0, Icon::Search, p.text3);
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut pane.search.query)
                                    .hint_text(RichText::new("搜索历史").color(p.text3))
                                    .frame(egui::Frame::NONE)
                                    .background_color(Color32::TRANSPARENT)
                                    .margin(vec2(6.0, 4.0))
                                    .desired_width(190.0),
                            );
                            if self.search_focus {
                                edit.request_focus();
                                self.search_focus = false;
                            }
                            let (enter, shift, esc) =
                                ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift, i.key_pressed(Key::Escape)));
                            let mut step = None;
                            if edit.changed() {
                                pane.search.reset();
                                step = Some(false);
                            }
                            if enter && edit.lost_focus() {
                                // 回车会让单行输入框失焦:找完把焦点还给输入框,方便连按
                                step = Some(shift);
                                edit.request_focus();
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if kit::icon_button_sized(ui, Icon::Close, false, 26.0, 12.0).on_hover_text("关闭  Esc").clicked() || esc {
                                    close = true;
                                }
                                let (sep, _) = ui.allocate_exact_size(vec2(9.0, 16.0), Sense::hover());
                                ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, p.border));
                                if kit::icon_button_sized(ui, Icon::ChevronDown, false, 26.0, 13.0).on_hover_text("更新的  ⇧⏎").clicked() {
                                    step = Some(true);
                                }
                                if kit::icon_button_sized(ui, Icon::ChevronUp, false, 26.0, 13.0).on_hover_text("更早的  ⏎").clicked() {
                                    step = Some(false);
                                }
                                if let Some(forward) = step {
                                    self.search_note = match pane.search.step(&pane.session, forward) {
                                        SearchResult::Found | SearchResult::Empty => None,
                                        SearchResult::NotFound => Some("没有结果".into()),
                                    };
                                }
                                if let Some(n) = &self.search_note {
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(n).font(style::small()).color(p.error));
                                }
                            });
                        });
                    });
            });
        if close {
            self.search_open = false;
            self.search_note = None;
            pane.session.term.lock().selection = None;
        }
    }

    // MARK: 状态栏

    pub(crate) fn status_bar(&mut self, ui: &mut Ui) {
        let p = palette(ui.ctx());
        let full = ui.max_rect();
        ui.painter().hline(full.min.x - 8.0..=full.max.x + 8.0, full.min.y - 0.5, Stroke::new(1.0, p.border));

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let modes = [VoiceMode::Dictation, VoiceMode::NaturalLanguage];
            let current = modes.iter().position(|m| *m == self.mode).unwrap_or(0);
            let seg = ui.scope(|ui| kit::segmented(ui, &[modes[0].label(), modes[1].label()], current));
            seg.response.on_hover_text(format!(
                "听写:原样上屏\n自然语言:说要做什么,生成 shell 命令\n切换  {}",
                Action::ToggleMode.label()
            ));
            if let Some(i) = seg.inner {
                self.mode = modes[i];
            }

            self.voice_capsule(ui, &p);
            self.status_message(ui, &p);

            let usage = self.active_session().and_then(|s| s.shared.usage());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if kit::icon_button(ui, Icon::Gear, self.show_settings, format!("设置  {}", Action::Settings.label())).clicked() {
                    self.show_settings = !self.show_settings;
                }
                if kit::icon_button(ui, Icon::Keyboard, self.show_help, format!("快捷键  {}", Action::Help.label())).clicked() {
                    self.show_help = !self.show_help;
                }
                if kit::icon_button(
                    ui,
                    Icon::Blocks,
                    self.show_loader,
                    format!("加载器  {}\n把设置里添加的文件夹中的技能、Agent、插件加载进当前目录", Action::Loader.label()),
                )
                .clicked()
                {
                    self.toggle_loader();
                }
                self.projects_menu(ui);
                if let Some(usage) = usage {
                    ui.add_space(8.0);
                    widgets::claude_meter(ui, &usage);
                }
            });
        });
    }

    /// 当前该显示的语音状态。测试截图时可以强制指定
    pub(crate) fn voice_ui(&self) -> VoiceUi {
        #[cfg(test)]
        if let Some(v) = &self.preview_voice {
            return v.clone();
        }
        if self.pipeline.is_recording() {
            let secs = self.rec_started.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
            VoiceUi::Recording { secs, levels: self.levels.iter().copied().collect() }
        } else if self.pipeline.is_busy() {
            VoiceUi::Working(self.status.text.clone())
        } else if self.config.voice.engine == VoiceEngineKind::Off {
            VoiceUi::Off
        } else {
            match vg_voice::build_engine(&self.config.voice) {
                Ok(e) => VoiceUi::Idle(pretty_engine(&e.name())),
                Err(e) => VoiceUi::Unconfigured(format!("{e:#}")),
            }
        }
    }

    fn voice_capsule(&mut self, ui: &mut Ui, p: &Palette) {
        let voice = self.voice_ui();
        let h = 24.0;
        let t = ui.input(|i| i.time) as f32;
        let font = style::body();
        let painter = ui.painter().clone();
        match voice {
            VoiceUi::Idle(engine) => {
                let resp = capsule(ui, h, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, h), Sense::hover());
                    icons::paint(ui.painter(), r, 14.0, Icon::Mic, p.text2);
                    ui.label(RichText::new("按住").color(p.text2));
                    kit::keycap(ui, "空格");
                    ui.label(RichText::new("说话").color(p.text2));
                    ui.label(RichText::new(engine).font(style::small()).color(p.text3));
                });
                if resp
                    .on_hover_text(format!("或点这里开始录音  {}", Action::ToggleRecording.label()))
                    .clicked()
                {
                    self.on_ptt(PttAction::StartRecording);
                }
            }
            VoiceUi::Unconfigured(why) => {
                let resp = capsule(ui, h, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, h), Sense::hover());
                    icons::paint(ui.painter(), r, 14.0, Icon::Alert, p.warn);
                    ui.label(RichText::new("还没有可用的语音引擎").color(p.warn));
                    ui.label(RichText::new("去设置").font(style::small()).color(p.text3));
                });
                if resp.on_hover_text(why).clicked() {
                    self.show_settings = true;
                }
            }
            VoiceUi::Off => {
                capsule(ui, h, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, h), Sense::hover());
                    icons::paint(ui.painter(), r, 14.0, Icon::Mic, p.text3);
                    ui.label(RichText::new("语音已关闭").color(p.text3));
                });
            }
            VoiceUi::Recording { secs, levels } => {
                let width = 12.0 + 10.0 + 6.0 + 44.0 + 8.0 + WAVE_BARS as f32 * 4.0 + 10.0 + 34.0 + 12.0;
                let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
                painter.rect_filled(rect, h / 2.0, alpha(p.rec, if p.dark { 0.16 } else { 0.10 }));
                painter.rect_stroke(rect, h / 2.0, Stroke::new(1.0, alpha(p.rec, 0.45)), egui::StrokeKind::Inside);
                let mut x = rect.min.x + 12.0;
                let cy = rect.center().y;
                let pulse = 0.55 + 0.45 * (t * 4.0).sin().abs();
                painter.circle_filled(pos2(x + 4.0, cy), 6.5, alpha(p.rec, 0.25 * pulse));
                painter.circle_filled(pos2(x + 4.0, cy), 4.0, p.rec);
                x += 16.0;
                let label = mix(p.rec, p.text, 0.35);
                painter.text(pos2(x, cy), Align2::LEFT_CENTER, "正在听", crate::fonts::medium(12.5), label);
                x += 50.0;
                for (i, level) in levels.iter().enumerate().take(WAVE_BARS) {
                    // 电平通常很小(说话 0.02~0.15),开方拉开层次
                    let v = (level * 7.0).clamp(0.0, 1.0).sqrt();
                    let bh = 2.0 + v * (h - 10.0);
                    let bx = x + i as f32 * 4.0;
                    let age = i as f32 / WAVE_BARS as f32;
                    painter.rect_filled(
                        Rect::from_center_size(pos2(bx + 1.0, cy), vec2(2.0, bh)),
                        1.0,
                        alpha(p.rec, 0.35 + 0.65 * age),
                    );
                }
                x += WAVE_BARS as f32 * 4.0 + 10.0;
                let time = format!("{}:{:02}", secs as u32 / 60, secs as u32 % 60);
                painter.text(pos2(x, cy), Align2::LEFT_CENTER, time, egui::FontId::monospace(11.5), p.text2);
                ui.ctx().request_repaint_after(Duration::from_millis(33));
                if resp.on_hover_text("松开空格结束;或点这里停止").clicked() {
                    self.on_ptt(PttAction::StopRecording);
                }
            }
            VoiceUi::Working(stage) => {
                capsule_filled(ui, h, alpha(p.accent, if p.dark { 0.14 } else { 0.10 }), |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(22.0, h), Sense::hover());
                    for k in 0..3 {
                        let phase = (t * 5.0 - k as f32 * 0.7).sin() * 0.5 + 0.5;
                        let c = pos2(r.min.x + 4.0 + k as f32 * 7.0, r.center().y - phase * 2.5);
                        ui.painter().circle_filled(c, 2.4, alpha(p.accent, 0.45 + 0.55 * phase));
                    }
                    let text = if stage.is_empty() { "识别中".to_string() } else { stage.trim_end_matches('…').to_string() };
                    ui.label(RichText::new(text).font(font.clone()).color(mix(p.accent, p.text, 0.4)));
                });
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        }
    }

    /// 上一次操作的结果(已上屏 / 出错),15 秒后淡出
    fn status_message(&mut self, ui: &mut Ui, p: &Palette) {
        if self.pipeline.is_busy() && !self.status.is_error {
            return; // 阶段文字已经在胶囊里了
        }
        let Some(at) = self.status.at else { return };
        let age = at.elapsed().as_secs_f32();
        let a = if self.status.is_error { 1.0 } else { ((STATUS_TTL - age) / STATUS_FADE).clamp(0.0, 1.0) };
        if a <= 0.0 || self.status.text.is_empty() {
            return;
        }
        if a < 1.0 {
            ui.ctx().request_repaint();
        } else if !self.status.is_error {
            ui.ctx().request_repaint_after(Duration::from_secs_f32(STATUS_TTL - STATUS_FADE - age + 0.05));
        }
        let (icon, color) = if self.status.is_error { (Icon::Alert, p.error) } else { (Icon::Check, p.done) };
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 24.0), Sense::hover());
        icons::paint(ui.painter(), r, 13.0, icon, alpha(color, a));
        // 超长截断,完整内容在悬停提示里
        let text_color = if self.status.is_error { p.error } else { p.text2 };
        let resp = ui.add(
            egui::Label::new(RichText::new(&self.status.text).color(alpha(text_color, a)))
                .truncate()
                .wrap_mode(egui::TextWrapMode::Truncate),
        );
        resp.on_hover_text(&self.status.text);
    }

    // MARK: 常用项目

    pub(crate) fn projects_menu(&mut self, ui: &mut Ui) {
        let p = palette(ui.ctx());
        let mut launch = None;
        let mut remove = None;
        let mut add = false;
        let resp = kit::icon_button(ui, Icon::Sparkle, false, "常用项目:进入目录并启动 Claude");
        egui::Popup::menu(&resp).width(260.0).gap(6.0).show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new("常用项目").font(crate::fonts::semibold(11.5)).color(p.text3));
            ui.add_space(2.0);
            let mut dirs = self.settings.claude_dirs.clone();
            dirs.sort_by_key(|d| (widgets::dir_name(d).to_lowercase(), d.clone()));
            if dirs.is_empty() {
                ui.label(RichText::new("还没有保存的目录").font(style::small()).color(p.text3));
            }
            for dir in dirs {
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                let hovered = resp.hovered();
                if hovered {
                    ui.painter().rect_filled(row, 6.0, p.hover);
                }
                let icon_rect = Rect::from_center_size(pos2(row.min.x + 14.0, row.center().y), vec2(16.0, 16.0));
                icons::paint(ui.painter(), icon_rect, 14.0, Icon::Folder, p.text2);
                ui.painter().text(pos2(row.min.x + 30.0, row.center().y), Align2::LEFT_CENTER, widgets::dir_name(&dir), style::body(), p.text);
                let trash = Rect::from_center_size(pos2(row.max.x - 14.0, row.center().y), vec2(22.0, 22.0));
                let t = ui.interact(trash, resp.id.with("trash"), Sense::click());
                if hovered || t.hovered() {
                    if t.hovered() {
                        ui.painter().rect_filled(trash, 5.0, p.pressed);
                    }
                    icons::paint(ui.painter(), trash, 13.0, Icon::Trash, if t.hovered() { p.error } else { p.text3 });
                }
                if t.on_hover_text("移除").clicked() {
                    remove = Some(dir.clone());
                } else if resp.on_hover_text(format!("cd {} && claude", dir.display())).clicked() {
                    launch = Some(dir.clone());
                    ui.close();
                }
            }
            ui.add_space(2.0);
            kit::hairline(ui);
            ui.add_space(2.0);
            let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(row, 6.0, p.hover);
            }
            let icon_rect = Rect::from_center_size(pos2(row.min.x + 14.0, row.center().y), vec2(16.0, 16.0));
            icons::paint(ui.painter(), icon_rect, 13.0, Icon::Plus, p.accent);
            ui.painter().text(pos2(row.min.x + 30.0, row.center().y), Align2::LEFT_CENTER, "添加目录…", style::body(), p.accent);
            if resp.clicked() {
                add = true;
                ui.close();
            }
        });

        if let Some(dir) = launch {
            if let Some(s) = self.active_session() {
                s.write(vg_core::shell::launch_claude_command(&dir).into_bytes());
            }
        }
        if let Some(dir) = remove {
            self.settings.claude_dirs.retain(|d| *d != dir);
            let _ = self.settings.save();
        }
        if add {
            if let Some(dir) =
                rfd::FileDialog::new().set_title("选择一个项目目录(点击菜单项会 cd 进入并启动 claude)").pick_folder()
            {
                if !self.settings.claude_dirs.contains(&dir) {
                    self.settings.claude_dirs.push(dir);
                    let _ = self.settings.save();
                }
            }
        }
    }

    // MARK: 浮窗

    pub(crate) fn help_window(&mut self, ctx: &egui::Context) {
        let p = palette(ctx);
        let mut close = false;
        let max_h = ctx.content_rect().height() - 120.0;
        kit::window("快捷键").anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(680.0);
            close = kit::window_header(ui, "快捷键", Some(&format!("随时按 {} 打开这张表", Action::Help.label())));
            let sections = shortcuts::help_sections();
            // 两栏按行数大致对半分
            let (left, right): (Vec<_>, Vec<_>) = sections.iter().partition(|(t, _)| ["语音", "标签页", "其他"].contains(t));
            egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| {
                ui.columns(2, |cols| {
                    for (col, list) in cols.iter_mut().zip([left, right]) {
                        for (title, rows) in list {
                            kit::section(col, title);
                            kit::card(col, |ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                for (keys, desc) in rows {
                                    ui.allocate_ui_with_layout(
                                        vec2(ui.available_width(), 24.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.label(RichText::new(*desc).color(p.text));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                shortcut_keys(ui, &p, keys);
                                            });
                                        },
                                    );
                                }
                            });
                            col.add_space(10.0);
                        }
                    }
                });
            });
        });
        if close {
            self.show_help = false;
        }
    }

    pub(crate) fn quit_dialog(&mut self, ctx: &egui::Context) {
        let p = palette(ctx);
        let frame = egui::Frame::NONE
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(14)
            .shadow(ctx.global_style().visuals.window_shadow)
            .inner_margin(22);
        egui::Modal::new(egui::Id::new("confirm-quit")).frame(frame).backdrop_color(Color32::from_black_alpha(110)).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new("退出 VoiceGhostty?").font(crate::fonts::semibold(16.0)).color(p.text));
            ui.add_space(4.0);
            let n = self.ws.panes.len();
            ui.label(RichText::new(format!("{n} 个窗格里的 shell,以及其中正在运行的程序,都会被挂断。")).color(p.text2));
            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::button(ui, "退出", kit::Kind::Danger, true).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.quit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if kit::button(ui, "取消", kit::Kind::Secondary, true).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.confirm_quit = false;
                }
            });
        });
    }
}

/// 胶囊底(悬停时出底色),内容横排
fn capsule(ui: &mut Ui, h: f32, content: impl FnOnce(&mut Ui)) -> egui::Response {
    let p = palette(ui.ctx());
    let id = ui.id().with("capsule");
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let fill = if hovered { p.hover } else { Color32::TRANSPARENT };
    let inner = capsule_filled(ui, h, fill, content);
    ui.interact(inner.rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn capsule_filled(ui: &mut Ui, h: f32, fill: Color32, content: impl FnOnce(&mut Ui)) -> egui::Response {
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same((h / 2.0) as u8))
        .inner_margin(egui::Margin::symmetric(10, 0))
        .show(ui, |ui| {
            ui.set_height(h);
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.horizontal_centered(content);
        })
        .response
}

enum KeyToken {
    Cap(String),
    Text(String),
}

/// 帮助表右侧的按键拆成一串:"⌘⇧] / ⌘⇧[" → 键帽 ⌘ ⇧ ] 、文字 /、键帽 ⌘ ⇧ [;
/// "按住空格" → 文字 按住、键帽 空格;"⌘⌥← 等方向键" → 键帽 + 文字;纯中文说明("双击标签")就是文字
fn key_tokens(keys: &str) -> Vec<KeyToken> {
    let is_cjk = |c: char| ('\u{4e00}'..='\u{9fff}').contains(&c);
    let caps = |s: &str| kit::split_keys(&s.replace('^', "⌃")).into_iter().map(KeyToken::Cap).collect::<Vec<_>>();
    let mut out = vec![];
    for (n, part) in keys.split(" / ").enumerate() {
        if n > 0 {
            out.push(KeyToken::Text("/".into()));
        }
        if let Some(rest) = part.strip_prefix("按住") {
            out.push(KeyToken::Text("按住".into()));
            out.push(KeyToken::Cap(rest.into()));
        } else if let Some((k, words)) = part.split_once(' ').filter(|(k, _)| !k.chars().any(is_cjk)) {
            out.extend(caps(k));
            out.push(KeyToken::Text(words.into()));
        } else if part.chars().any(is_cjk) {
            out.push(KeyToken::Text(part.into()));
        } else {
            out.extend(caps(part));
        }
    }
    out
}

/// 在 right_to_left 布局里倒着放,屏幕上是正序
fn shortcut_keys(ui: &mut Ui, p: &Palette, keys: &str) {
    ui.spacing_mut().item_spacing.x = 3.0;
    for token in key_tokens(keys).into_iter().rev() {
        match token {
            KeyToken::Cap(k) => {
                kit::keycap(ui, &k);
            }
            KeyToken::Text(t) => {
                ui.label(RichText::new(t).font(style::small()).color(p.text3));
            }
        }
    }
}

/// "whisper · ggml-large-v3-turbo.bin" → "whisper · large-v3-turbo"
fn pretty_engine(name: &str) -> String {
    name.replace("ggml-", "").replace(".bin", "")
}

#[cfg(test)]
mod tests {
    use super::{key_tokens, KeyToken};

    fn show(keys: &str) -> String {
        key_tokens(keys)
            .into_iter()
            .map(|t| match t {
                KeyToken::Cap(k) => format!("[{k}]"),
                KeyToken::Text(t) => t,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn help_keys_become_caps_and_words() {
        assert_eq!(show("⌘⇧] / ⌘⇧["), "[⌘] [⇧] []] / [⌘] [⇧] [[]");
        assert_eq!(show("按住空格"), "按住 [空格]");
        assert_eq!(show("⌘⌥← 等方向键"), "[⌘] [⌥] [←] 等方向键");
        assert_eq!(show("双击标签"), "双击标签");
        assert_eq!(show("⌘^1…⌘^8"), "[⌘⌃1…⌘⌃8]");
        assert_eq!(show("Ctrl+Shift+D"), "[Ctrl] [Shift] [D]");
    }
}
