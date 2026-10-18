//! 设置窗口。GUI 设置存 settings.json,覆盖配置文件里的同名项;其余选项仍写在配置文件里。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense};

use vg_core::settings::{clamp_scrollback, SCROLLBACK_RANGE};
use vg_core::{Theme, VoiceEngineKind};

use crate::app::VoiceGhosttyApp;
use crate::icons::{self, Icon};
use crate::kit;
use crate::style::{self, palette};
use crate::widgets;

const WIDTH: f32 = 470.0;

impl VoiceGhosttyApp {
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        let p = palette(ctx);
        let mut changed = false;
        let mut close = false;
        kit::window("设置").anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(WIDTH);
            close = kit::window_header(ui, "设置", Some("改动立即生效"));

            // 内容放不下时滚动区也占满可用高度(默认只给 64px 起步,窗口会被压得很矮)
            let max_h = ctx.content_rect().height() - 140.0;
            egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| {
                ui.set_width(WIDTH);
                kit::section(ui, "语音");
                kit::card(ui, |ui| {
                    kit::setting_row(ui, "语音引擎", Some(engine_hint(self.config.voice.engine)), |ui| {
                        let current = self.config.voice.engine;
                        egui::ComboBox::from_id_salt("engine").width(150.0).selected_text(engine_label(current)).show_ui(ui, |ui| {
                            for kind in [VoiceEngineKind::Auto, VoiceEngineKind::Whisper, VoiceEngineKind::Http, VoiceEngineKind::Command, VoiceEngineKind::Off] {
                                if ui.selectable_label(current == kind, engine_label(kind)).clicked() {
                                    self.settings.voice_engine = Some(kind.as_str().into());
                                    changed = true;
                                }
                            }
                        });
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "识别语言", None, |ui| {
                        let lang = self.config.voice.language.clone();
                        egui::ComboBox::from_id_salt("lang").width(150.0).selected_text(widgets::language_label(&lang)).show_ui(ui, |ui| {
                            for code in ["zh", "en", "auto"] {
                                if ui.selectable_label(lang == code, widgets::language_label(code)).clicked() {
                                    self.settings.default_language = Some(code.into());
                                    changed = true;
                                }
                            }
                        });
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "自动标点", Some("主要口述 shell 命令时建议关掉"), |ui| {
                        let mut punct = self.config.voice.punctuation;
                        if toggle(ui, &mut punct).changed() {
                            self.settings.adds_punctuation = Some(punct);
                            changed = true;
                        }
                    });
                });

                ui.add_space(10.0);
                kit::section(ui, "终端");
                kit::card(ui, |ui| {
                    kit::setting_row(ui, "完成提示音", Some("命令跑完、等你处理时响一声"), |ui| {
                        if toggle(ui, &mut self.settings.done_sound).changed() {
                            self.monitor.chime_enabled.store(self.settings.done_sound, Ordering::Relaxed);
                            changed = true;
                        }
                        if kit::icon_button_sized(ui, Icon::Play, false, 26.0, 13.0).on_hover_text("试听").clicked() {
                            vg_voice::chime::play();
                        }
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "保存会话记录", Some("对之后新开的窗格生效"), |ui| {
                        if toggle(ui, &mut self.settings.save_history).changed() {
                            changed = true;
                        }
                        if kit::icon_button_sized(ui, Icon::Folder, false, 26.0, 13.0).on_hover_text("打开记录文件夹").clicked() {
                            widgets::open_in_file_manager(&vg_core::paths::history_dir());
                        }
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "默认文件夹", Some("新标签页和启动时从这里开始;分屏跟随原窗格"), |ui| {
                        let current = self.settings.default_dir.clone();
                        let shown = current.as_deref().map(vg_core::loader::tilde).unwrap_or_else(|| "家目录".into());
                        let full = current.as_deref().map(|d| d.display().to_string()).unwrap_or_default();
                        if current.is_some() {
                            if kit::icon_button_sized(ui, Icon::Close, false, 26.0, 13.0).on_hover_text("恢复为家目录").clicked() {
                                self.settings.default_dir = None;
                                changed = true;
                            }
                        }
                        if kit::icon_button_sized(ui, Icon::Folder, false, 26.0, 13.0).on_hover_text("选择文件夹").clicked() {
                            let start = current.clone().unwrap_or_else(vg_core::paths::home_dir);
                            if let Some(picked) = rfd::FileDialog::new().set_title("选择新标签页的默认文件夹").set_directory(start).pick_folder() {
                                self.settings.default_dir = Some(picked);
                                changed = true;
                            }
                        }
                        let label = ui.add(egui::Label::new(RichText::new(shown).font(style::small()).color(p.text2)).truncate());
                        if !full.is_empty() {
                            label.on_hover_text(full);
                        }
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "历史行数", Some("每个窗格保留多少行滚出屏幕的输出"), |ui| {
                        let mut lines = self.config.scrollback;
                        let drag = egui::DragValue::new(&mut lines).range(SCROLLBACK_RANGE).speed(100.0).suffix(" 行");
                        if ui.add_sized(vec2(96.0, 24.0), drag).changed() {
                            let lines = clamp_scrollback(lines);
                            self.settings.scrollback = Some(lines);
                            for pane in self.ws.panes.values() {
                                pane.session.set_scrollback(lines);
                            }
                            changed = true;
                        }
                    });
                });

                ui.add_space(10.0);
                kit::section(ui, "外观");
                kit::card(ui, |ui| {
                    kit::setting_row(ui, "主题", Some("本次运行有效;长期修改写在配置文件"), |ui| {
                        let theme_id = self.config.theme_name.clone();
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // right_to_left:倒着放,屏幕上从左到右是 ALL 的顺序
                        for t in Theme::ALL.iter().rev() {
                            if theme_swatch(ui, t, t.id == theme_id).clicked() {
                                self.config.theme_name = t.id.into();
                                style::apply(ui.ctx(), &self.config.theme());
                            }
                        }
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "命令输入颜色", Some("正在输入的命令行用这个颜色"), |ui| {
                        let mut color = vg_core::Rgb::parse(&self.settings.input_color)
                            .map(|c| Color32::from_rgb(c.0, c.1, c.2))
                            .unwrap_or(Color32::LIGHT_BLUE);
                        if ui.color_edit_button_srgba(&mut color).changed() {
                            self.settings.input_color = vg_core::Rgb(color.r(), color.g(), color.b()).to_hex();
                            changed = true;
                        }
                    });
                });

                ui.add_space(10.0);
                kit::section(ui, "加载文件夹");
                if self.library_section(ui) {
                    changed = true;
                }

                ui.add_space(12.0);
                let path = vg_core::paths::config_file();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("API 地址、外部识别命令、模型等更多选项写在").font(style::small()).color(p.text3));
                    let link = ui.add(
                        egui::Label::new(RichText::new(vg_core::loader::tilde(&path)).font(style::small()).color(p.accent))
                            .sense(Sense::click()),
                    );
                    if link.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("在文件管理器里显示").clicked() {
                        if let Some(dir) = path.parent() {
                            widgets::open_in_file_manager(dir);
                        }
                    }
                });
                // 版本 + 提交号:一眼确认跑的是哪个构建(市场版本跨构建不变,单看它分不出新旧)
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("VoiceGhostty {}", crate::version::label())).font(style::small()).color(p.text3))
                        .on_hover_text(crate::version::detail());
                });
            });
        });
        if close {
            self.show_settings = false;
        }
        if changed {
            let theme_name = self.config.theme_name.clone();
            self.reload_config(ctx);
            self.config.theme_name = theme_name; // 主题选择本次会话内有效(配置文件为准)
            style::apply(ctx, &self.config.theme());
        }
    }

    /// 设置页的"加载文件夹"列表:顺序决定重名时谁优先
    fn library_section(&mut self, ui: &mut egui::Ui) -> bool {
        let p = palette(ui.ctx());
        let mut dirs = self.config.skill_library_dirs.clone();
        let mut changed = false;
        let mut action: Option<(usize, bool)> = None; // (下标, true=删除 / false=上移)
        kit::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(
                RichText::new("加载器从这些文件夹识别技能、Agent、插件;同名时靠上的优先")
                    .font(style::small())
                    .color(p.text3),
            );
            ui.add_space(2.0);
            for (i, dir) in dirs.iter().enumerate() {
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                let hovered = resp.hovered();
                if hovered {
                    ui.painter().rect_filled(row.expand2(vec2(6.0, 0.0)), 6.0, p.hover);
                }
                let icon = Rect::from_center_size(pos2(row.min.x + 9.0, row.center().y), vec2(16.0, 16.0));
                icons::paint(ui.painter(), icon, 14.0, Icon::Folder, p.text2);
                ui.painter().text(
                    pos2(row.min.x + 26.0, row.center().y),
                    Align2::LEFT_CENTER,
                    vg_core::loader::tilde(dir),
                    style::body(),
                    p.text,
                );
                let order = format!("{}", i + 1);
                let mut x = row.max.x;
                let mut btn = |icon: Icon, tip: &str, danger: bool, ui: &mut egui::Ui| -> bool {
                    let r = Rect::from_center_size(pos2(x - 12.0, row.center().y), vec2(24.0, 24.0));
                    x -= 26.0;
                    let b = ui.interact(r, resp.id.with(tip), Sense::click());
                    if b.hovered() {
                        ui.painter().rect_filled(r, 5.0, p.pressed);
                    }
                    let color = if b.hovered() && danger { p.error } else if b.hovered() { p.text } else { p.text3 };
                    icons::paint(ui.painter(), r, 13.0, icon, color);
                    b.on_hover_text(tip).clicked()
                };
                if hovered || ui.rect_contains_pointer(row) {
                    if btn(Icon::Trash, "移除", true, ui) {
                        action = Some((i, true));
                    }
                    if i > 0 && btn(Icon::ArrowUp, "上移(优先级更高)", false, ui) {
                        action = Some((i, false));
                    }
                } else {
                    ui.painter().text(pos2(row.max.x - 4.0, row.center().y), Align2::RIGHT_CENTER, order, crate::fonts::medium(11.0), p.text3);
                }
                resp.on_hover_text(dir.display().to_string());
            }
            if !dirs.is_empty() {
                ui.add_space(2.0);
            }
            if kit::button(ui, "添加文件夹…", kit::Kind::Secondary, true).clicked() {
                let start = dirs.last().cloned().unwrap_or_else(vg_core::paths::home_dir);
                if let Some(picked) = rfd::FileDialog::new()
                    .set_title("选择要加载的文件夹 —— 其中的插件、技能、Agent 会自动识别(可多选)")
                    .set_directory(start)
                    .pick_folders()
                {
                    let added: Vec<PathBuf> = picked.into_iter().filter(|p| !dirs.contains(p)).collect();
                    if !added.is_empty() {
                        dirs.extend(added.iter().cloned());
                        self.library_notice = widgets::duplicate_notice(&added, &dirs);
                        changed = true;
                    }
                }
            }
            if let Some(n) = &self.library_notice {
                ui.label(RichText::new(n).font(style::small()).color(p.warn));
            }
        });
        match action {
            Some((i, true)) => {
                dirs.remove(i);
                self.library_notice = None; // 列表变了,之前的提示未必还成立
                changed = true;
            }
            Some((i, false)) => {
                dirs.swap(i - 1, i);
                changed = true;
            }
            None => {}
        }
        if changed {
            self.settings.skill_library_dirs = Some(dirs);
        }
        changed
    }
}

fn engine_label(kind: VoiceEngineKind) -> &'static str {
    match kind {
        VoiceEngineKind::Auto => "自动",
        VoiceEngineKind::Whisper => "内置 whisper",
        VoiceEngineKind::Http => "HTTP 接口",
        VoiceEngineKind::Command => "外部命令",
        VoiceEngineKind::Off => "关闭",
    }
}

fn engine_hint(kind: VoiceEngineKind) -> &'static str {
    match kind {
        VoiceEngineKind::Auto => "按可用性挑:内置 whisper → 外部命令 → HTTP",
        VoiceEngineKind::Whisper => "本地离线,模型放在 models/ 目录",
        VoiceEngineKind::Http => "OpenAI 兼容的转写接口",
        VoiceEngineKind::Command => "调用 voice-command 配置的程序",
        VoiceEngineKind::Off => "按住空格就是空格",
    }
}

/// iOS 风格的开关
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let p = palette(ui.ctx());
    let size = vec2(34.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.14);
    let track = style::mix(p.pressed, p.accent, t);
    ui.painter().rect_filled(rect, 10.0, track);
    let x = egui::lerp(rect.min.x + 10.0..=rect.max.x - 10.0, t);
    let knob = pos2(x, rect.center().y);
    ui.painter().circle_filled(knob + vec2(0.0, 0.6), 8.0, Color32::from_black_alpha(50));
    ui.painter().circle_filled(knob, 8.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 主题色块:终端底色 + 前景色的一道,选中描强调色
fn theme_swatch(ui: &mut egui::Ui, theme: &Theme, selected: bool) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 22.0), Sense::click());
    let c = |rgb: vg_core::Rgb| Color32::from_rgb(rgb.0, rgb.1, rgb.2);
    let inner = rect.shrink(if selected { 2.5 } else { 1.0 });
    ui.painter().rect_filled(inner, 5.0, c(theme.background));
    for (i, k) in [1usize, 2, 4].iter().enumerate() {
        let r = Rect::from_min_size(pos2(inner.min.x + 5.0 + i as f32 * 6.0, inner.center().y - 2.0), vec2(4.0, 4.0));
        ui.painter().rect_filled(r, 1.0, c(theme.ansi[*k]));
    }
    ui.painter().hline(inner.min.x + 5.0..=inner.max.x - 5.0, inner.center().y + 5.0, egui::Stroke::new(1.5, c(theme.foreground)));
    let stroke = if selected {
        egui::Stroke::new(1.5, p.accent)
    } else {
        egui::Stroke::new(1.0, if resp.hovered() { p.text3 } else { p.border })
    };
    ui.painter().rect_stroke(rect.shrink(if selected { 0.75 } else { 0.5 }), 6.5, stroke, egui::StrokeKind::Middle);
    resp.on_hover_text(theme.display_name).on_hover_cursor(egui::CursorIcon::PointingHand)
}
