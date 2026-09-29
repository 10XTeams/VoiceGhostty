//! 主窗口:标签栏 + 终端 + 状态栏(语音模式、引擎、录音电平、结果说明)+ 设置窗。

use std::time::{Duration, Instant};

use egui::{Color32, Key, KeyboardShortcut, Modifiers, RichText};

use vg_core::{Config, Settings, Theme, VoiceEngineKind};

use crate::pipeline::{Pipeline, PipelineEvent, VoiceMode};
use crate::ptt::PttAction;
use crate::terminal::session::{GridSize, TerminalSession};
use crate::terminal::view::{self, InputState, ViewOptions};

struct Tab {
    session: TerminalSession,
    input: InputState,
}

#[derive(Default)]
struct Status {
    text: String,
    is_error: bool,
    at: Option<Instant>,
}

pub struct VoiceGhosttyApp {
    config: Config,
    settings: Settings,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    mode: VoiceMode,
    pipeline: Pipeline,
    status: Status,
    font_size: f32,
    show_settings: bool,
}

/// 应用快捷键的修饰键:macOS 用 ⌘;其他平台用 Ctrl+Shift(单 Ctrl 组合要留给终端,如 Ctrl+T/W)
fn app_mods() -> Modifiers {
    if cfg!(target_os = "macos") { Modifiers::MAC_CMD } else { Modifiers::CTRL | Modifiers::SHIFT }
}

pub fn effective_config(settings: &Settings) -> Config {
    let mut config = Config::load();
    settings.apply_to(&mut config);
    config
}

impl VoiceGhosttyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> VoiceGhosttyApp {
        let settings = Settings::load();
        let config = effective_config(&settings);
        crate::fonts::install(&cc.egui_ctx, &config.font_family);
        apply_visuals(&cc.egui_ctx, &config.theme());

        // 把 Ollama 冷加载挪到用户开口之前;服务没开会立即失败,无副作用
        if config.correction_enabled {
            let (model, url) = (config.llm_local_model.clone(), config.llm_local_url.clone());
            std::thread::spawn(move || vg_core::ollama::warm_up(&model, &url));
        }

        let mut app = VoiceGhosttyApp {
            font_size: config.font_size,
            config,
            settings,
            tabs: vec![],
            active: 0,
            next_id: 1,
            mode: VoiceMode::Dictation,
            pipeline: Pipeline::new(),
            status: Status::default(),
            show_settings: false,
        };
        app.new_tab(&cc.egui_ctx);
        app
    }

    fn new_tab(&mut self, ctx: &egui::Context) {
        // 初始尺寸随便给,首帧布局时会按实际区域 resize
        let size = GridSize { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
        match TerminalSession::spawn(ctx, size, self.config.scrollback, &self.settings, self.next_id) {
            Ok(session) => {
                self.next_id += 1;
                self.tabs.push(Tab { session, input: InputState::default() });
                self.active = self.tabs.len() - 1;
            }
            Err(e) => self.set_status(format!("无法启动 shell:{e:#}"), true),
        }
    }

    fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Status { text: text.into(), is_error, at: Some(Instant::now()) };
    }

    fn reload_config(&mut self, ctx: &egui::Context) {
        self.config = effective_config(&self.settings);
        apply_visuals(ctx, &self.config.theme());
        if let Err(e) = self.settings.save() {
            self.set_status(format!("设置保存失败:{e}"), true);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mods = app_mods();
        let pressed = |key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key)));
        if pressed(Key::T) {
            self.new_tab(ctx);
        }
        if pressed(Key::W) && !self.tabs.is_empty() {
            self.tabs.remove(self.active);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
        if pressed(Key::Equals) || pressed(Key::Plus) {
            self.font_size = (self.font_size + 1.0).min(40.0);
        }
        if pressed(Key::Minus) {
            self.font_size = (self.font_size - 1.0).max(8.0);
        }
        if pressed(Key::Num0) {
            self.font_size = self.config.font_size;
        }
        if pressed(Key::M) {
            self.mode = match self.mode {
                VoiceMode::Dictation => VoiceMode::NaturalLanguage,
                VoiceMode::NaturalLanguage => VoiceMode::Dictation,
            };
        }
        if pressed(Key::Comma) {
            self.show_settings = !self.show_settings;
        }
        let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        for (i, key) in digits.into_iter().enumerate() {
            if pressed(key) && i < self.tabs.len() {
                self.active = i;
            }
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        let theme = self.config.theme();
        let mut clipboard = None;
        for tab in &mut self.tabs {
            tab.session.pump_events(&theme, &mut clipboard);
        }
        if let Some(text) = clipboard {
            ctx.copy_text(text);
        }
        // shell 退出的标签页自动关闭;全部关完就退出
        let before = self.tabs.len();
        self.tabs.retain(|t| !t.session.exited);
        if self.tabs.len() != before {
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
            if self.tabs.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        while let Ok(event) = self.pipeline.rx.try_recv() {
            match event {
                PipelineEvent::Stage(s) => self.set_status(s, false),
                PipelineEvent::Insert { text, explanation } => {
                    self.pipeline.finished();
                    if let Some(tab) = self.tabs.get(self.active) {
                        tab.session.type_text(&text);
                    }
                    let note = explanation.unwrap_or_else(|| format!("已上屏:{text}"));
                    self.set_status(note, false);
                }
                PipelineEvent::Nothing => {
                    self.pipeline.finished();
                    self.set_status("没听到声音", false);
                }
                PipelineEvent::Error(e) => {
                    self.pipeline.finished();
                    self.set_status(e, true);
                }
            }
        }
    }

    fn on_ptt(&mut self, action: PttAction) {
        match action {
            PttAction::StartRecording => match self.pipeline.start(&self.config) {
                Ok(()) => self.set_status("正在听…松开空格结束", false),
                Err(e) => self.set_status(e, true),
            },
            PttAction::StopRecording => {
                let cwd = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.session.current_directory())
                    .map(|p| p.to_string_lossy().into_owned());
                self.pipeline.stop(&self.config, self.mode, cwd);
            }
            _ => {}
        }
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mut close = None;
            for (i, tab) in self.tabs.iter().enumerate() {
                let title = if tab.session.title.is_empty() { "shell".into() } else { tab.session.title.clone() };
                let label = format!("{} {}", i + 1, truncate(&title, 24));
                let r = ui.selectable_label(i == self.active, label);
                if r.clicked() {
                    self.active = i;
                }
                if r.middle_clicked() {
                    close = Some(i);
                }
            }
            if ui.button("＋").on_hover_text("新标签页").clicked() {
                self.new_tab(ui.ctx());
            }
            if let Some(i) = close {
                self.tabs.remove(i);
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for mode in [VoiceMode::Dictation, VoiceMode::NaturalLanguage] {
                if ui.selectable_label(self.mode == mode, mode.label()).clicked() {
                    self.mode = mode;
                }
            }
            ui.separator();

            if self.pipeline.is_recording() {
                let level = (self.pipeline.level() * 8.0).clamp(0.0, 1.0);
                ui.label(RichText::new("● REC").color(Color32::from_rgb(230, 70, 70)));
                ui.add(egui::ProgressBar::new(level).desired_width(80.0).desired_height(8.0));
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            } else if self.pipeline.is_busy() {
                ui.spinner();
            } else {
                let engine = match vg_voice::build_engine(&self.config.voice) {
                    Ok(e) => e.name(),
                    Err(_) if self.config.voice.engine == VoiceEngineKind::Off => "语音已关闭".into(),
                    Err(_) => "未配置语音引擎".into(),
                };
                ui.label(RichText::new(format!("🎤 按住空格说话 · {engine}")).weak());
            }
            ui.separator();

            // 状态文字 15 秒后淡出(错误保留到下一次操作)
            let fresh = self.status.at.is_some_and(|t| t.elapsed() < Duration::from_secs(15));
            if fresh || self.status.is_error {
                let color = if self.status.is_error { Color32::from_rgb(230, 90, 90) } else { ui.visuals().text_color() };
                ui.label(RichText::new(&self.status.text).color(color));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙").on_hover_text("设置").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
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
                egui::ComboBox::from_id_salt("lang").selected_text(language_label(&lang)).show_ui(ui, |ui| {
                    for code in ["zh", "en", "auto"] {
                        if ui.selectable_label(lang == code, language_label(code)).clicked() {
                            self.settings.default_language = Some(code.into());
                            changed = true;
                        }
                    }
                });
                ui.end_row();

                ui.label("自动标点");
                let mut punct = self.config.voice.punctuation;
                if ui.checkbox(&mut punct, "").changed() {
                    self.settings.adds_punctuation = Some(punct);
                    changed = true;
                }
                ui.end_row();

                ui.label("主题");
                let theme_id = self.config.theme_name.clone();
                egui::ComboBox::from_id_salt("theme").selected_text(Theme::named(&theme_id).display_name).show_ui(ui, |ui| {
                    for t in Theme::ALL {
                        if ui.selectable_label(t.id == theme_id, t.display_name).clicked() {
                            self.config.theme_name = t.id.into();
                            apply_visuals(ui.ctx(), &self.config.theme());
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
            apply_visuals(ctx, &self.config.theme());
        }
    }
}

impl eframe::App for VoiceGhosttyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        self.pump(&ctx);

        if let Some(tab) = self.tabs.get(self.active) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — VoiceGhostty", tab.session.title)));
        }

        egui::Panel::top("tabs").show(ui, |ui| self.tab_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        let theme = self.config.theme();
        let opts = ViewOptions {
            theme: &theme,
            font_size: self.font_size,
            hold: Duration::from_millis(self.config.voice.hold_ms),
            ptt_enabled: self.config.voice.engine != VoiceEngineKind::Off && !self.pipeline.is_busy(),
        };
        let mut actions = vec![];
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(theme.background.0, theme.background.1, theme.background.2)).inner_margin(4.0))
            .show(ui, |ui| {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    actions = view::show(ui, &mut tab.session, &mut tab.input, &opts);
                }
            });
        for action in actions {
            self.on_ptt(action);
        }

        if self.show_settings {
            self.settings_window(&ctx);
        }
    }
}

fn apply_visuals(ctx: &egui::Context, theme: &Theme) {
    let bg = theme.background;
    let luminance = 0.299 * bg.0 as f32 + 0.587 * bg.1 as f32 + 0.114 * bg.2 as f32;
    ctx.set_visuals(if luminance > 128.0 { egui::Visuals::light() } else { egui::Visuals::dark() });
}

fn language_label(code: &str) -> &'static str {
    match code {
        "zh" => "中文",
        "en" => "English",
        _ => "自动检测",
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).chain(std::iter::once('…')).collect()
    }
}
