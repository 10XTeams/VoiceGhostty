//! 设置窗口。GUI 设置存 settings.json,覆盖配置文件里的同名项;其余选项仍写在配置文件里。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use egui::{Color32, RichText};

use vg_core::settings::{clamp_scrollback, SCROLLBACK_RANGE};
use vg_core::{Theme, VoiceEngineKind};

use crate::app::VoiceGhosttyApp;
use crate::widgets::{self, WARN};

impl VoiceGhosttyApp {
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut changed = false;
        egui::Window::new("设置").open(&mut open).resizable(false).show(ctx, |ui| {
            egui::Grid::new("settings-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("语音引擎");
                let current = self.config.voice.engine;
                egui::ComboBox::from_id_salt("engine").selected_text(current.as_str()).show_ui(ui, |ui| {
                    for kind in [VoiceEngineKind::Auto, VoiceEngineKind::Http, VoiceEngineKind::Command, VoiceEngineKind::Off] {
                        if ui.selectable_label(current == kind, kind.as_str()).clicked() {
                            self.settings.voice_engine = Some(kind.as_str().into());
                            changed = true;
                        }
                    }
                });
                ui.end_row();

                ui.label("识别语言");
                let lang = self.config.voice.language.clone();
                egui::ComboBox::from_id_salt("lang").selected_text(widgets::language_label(&lang)).show_ui(ui, |ui| {
                    for code in ["zh", "en", "auto"] {
                        if ui.selectable_label(lang == code, widgets::language_label(code)).clicked() {
                            self.settings.default_language = Some(code.into());
                            changed = true;
                        }
                    }
                });
                ui.end_row();

                ui.label("自动标点");
                let mut punct = self.config.voice.punctuation;
                if ui
                    .checkbox(&mut punct, "")
                    .on_hover_text("口述给 Claude 的句子有标点好读得多;主要口述 shell 命令的话关掉,句末的句号只会碍事")
                    .changed()
                {
                    self.settings.adds_punctuation = Some(punct);
                    changed = true;
                }
                ui.end_row();

                ui.label("完成提示音");
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.settings.done_sound, "").changed() {
                        self.monitor.chime_enabled.store(self.settings.done_sound, Ordering::Relaxed);
                        changed = true;
                    }
                    if ui.small_button("试听").clicked() {
                        vg_voice::chime::play();
                    }
                });
                ui.end_row();

                ui.label("保存会话记录");
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.settings.save_history, "").on_hover_text("对之后新开的窗格生效").changed() {
                        changed = true;
                    }
                    if ui.small_button("打开文件夹").clicked() {
                        widgets::open_in_file_manager(&vg_core::paths::history_dir());
                    }
                });
                ui.end_row();

                ui.label("历史行数");
                let mut lines = self.config.scrollback;
                let drag = egui::DragValue::new(&mut lines)
                    .range(SCROLLBACK_RANGE)
                    .speed(100.0)
                    .suffix(" 行");
                if ui
                    .add(drag)
                    .on_hover_text("每个窗格保留多少行滚出屏幕的输出(⌘F 与滚动条能看到的范围)。改了立即作用于已打开的窗格")
                    .changed()
                {
                    let lines = clamp_scrollback(lines);
                    self.settings.scrollback = Some(lines);
                    for pane in self.ws.panes.values() {
                        pane.session.set_scrollback(lines);
                    }
                    changed = true;
                }
                ui.end_row();

                ui.label("主题");
                let theme_id = self.config.theme_name.clone();
                egui::ComboBox::from_id_salt("theme").selected_text(Theme::named(&theme_id).display_name).show_ui(ui, |ui| {
                    for t in Theme::ALL {
                        if ui.selectable_label(t.id == theme_id, t.display_name).clicked() {
                            self.config.theme_name = t.id.into();
                            widgets::apply_visuals(ui.ctx(), &self.config.theme());
                        }
                    }
                });
                ui.end_row();

                ui.label("命令输入颜色");
                let mut color = vg_core::Rgb::parse(&self.settings.input_color)
                    .map(|c| Color32::from_rgb(c.0, c.1, c.2))
                    .unwrap_or(Color32::LIGHT_BLUE);
                if ui.color_edit_button_srgba(&mut color).changed() {
                    self.settings.input_color = vg_core::Rgb(color.r(), color.g(), color.b()).to_hex();
                    changed = true;
                }
                ui.end_row();
            });
            ui.separator();
            if self.library_section(ui) {
                changed = true;
            }
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "更多选项(API 地址、外部识别命令、模型等)写在配置文件:\n{}",
                    vg_core::paths::config_file().display()
                ))
                .small()
                .weak(),
            );
        });
        self.show_settings = open;
        if changed {
            let theme_name = self.config.theme_name.clone();
            self.reload_config(ctx);
            self.config.theme_name = theme_name; // 主题选择本次会话内有效(配置文件为准)
            widgets::apply_visuals(ctx, &self.config.theme());
        }
    }

    /// 设置页的"加载文件夹"列表:顺序决定重名时谁优先
    fn library_section(&mut self, ui: &mut egui::Ui) -> bool {
        let mut dirs = self.config.skill_library_dirs.clone();
        let mut changed = false;
        ui.label(RichText::new("加载文件夹(🧩 面板从这些文件夹里识别插件、技能、Agent;靠上的优先)").strong());
        let mut action: Option<(usize, bool)> = None; // (下标, true=删除 / false=上移)
        for (i, dir) in dirs.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(vg_core::loader::tilde(dir)).on_hover_text(dir.display().to_string());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗑").on_hover_text("移除").clicked() {
                        action = Some((i, true));
                    }
                    if i > 0 && ui.small_button("↑").on_hover_text("上移").clicked() {
                        action = Some((i, false));
                    }
                });
            });
        }
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
        if ui.button("＋ 添加文件夹…").clicked() {
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
            ui.colored_label(WARN, RichText::new(n).small());
        }
        if changed {
            self.settings.skill_library_dirs = Some(dirs);
        }
        changed
    }
}
