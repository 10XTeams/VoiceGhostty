//! 主窗口:标签栏 + 分屏终端 + 搜索栏 + 状态栏(语音模式、引擎、录音电平、结果说明、claude 用量)
//! 以及设置 / 🧩 加载器 / 快捷键帮助 / 退出确认这几个浮窗。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use egui::{Color32, Key, RichText};

use vg_core::{Config, Settings, VoiceEngineKind};

use crate::loader_panel::LoaderPanel;
use crate::monitor::Monitor;
use crate::pipeline::{Pipeline, PipelineEvent, VoiceMode};
use crate::ptt::PttAction;
use crate::shortcuts::{self, Action};
use crate::split::{Axis, Dir};
use crate::terminal::search::{SearchResult, SearchState};
use crate::terminal::session::{GridSize, TerminalSession};
use crate::terminal::view::{self, InputState, ViewOptions};
use crate::widgets::{self, ERROR};
use crate::workspace::Workspace;

pub(crate) struct Pane {
    pub session: TerminalSession,
    pub input: InputState,
    pub search: SearchState,
}

#[derive(Default)]
pub(crate) struct Status {
    text: String,
    is_error: bool,
    at: Option<Instant>,
}

/// 分屏之间的间隙(露出底色即分隔线)
const SPLIT_GAP: f32 = 2.0;
const FONT_MIN: f32 = 8.0;
const FONT_MAX: f32 = 48.0;

pub struct VoiceGhosttyApp {
    pub(crate) config: Config,
    pub(crate) settings: Settings,
    pub(crate) ws: Workspace<Pane>,
    mode: VoiceMode,
    pipeline: Pipeline,
    status: Status,
    font_size: f32,
    pub(crate) show_settings: bool,
    pub(crate) monitor: Monitor,
    loader: LoaderPanel,
    show_loader: bool,
    /// 延时依次打进终端的命令。claude 执行第一条斜杠命令时第二条若到达,会停在输入框里不提交,所以要间隔开
    pending_commands: VecDeque<(Instant, String)>,
    /// 设置页添加文件夹后的重名提示
    pub(crate) library_notice: Option<String>,
    search_open: bool,
    search_focus: bool,
    search_note: Option<String>,
    /// 正在改名的标签页(下标, 草稿)
    renaming: Option<(usize, String)>,
    show_help: bool,
    confirm_quit: bool,
    quit_confirmed: bool,
}

pub fn effective_config(settings: &Settings) -> Config {
    let mut config = Config::load();
    settings.apply_to(&mut config);
    config
}

impl VoiceGhosttyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> VoiceGhosttyApp {
        crate::monitor::clean_stale_state_files();
        let settings = Settings::load();
        let config = effective_config(&settings);
        crate::fonts::install(&cc.egui_ctx, &config.font_family);
        widgets::apply_visuals(&cc.egui_ctx, &config.theme());

        vg_voice::preload(&config.voice);
        // 把 Ollama 冷加载挪到用户开口之前;服务没开会立即失败,无副作用
        if config.correction_enabled {
            let (model, url) = (config.llm_local_model.clone(), config.llm_local_url.clone());
            std::thread::spawn(move || vg_core::ollama::warm_up(&model, &url));
        }

        let mut app = VoiceGhosttyApp {
            font_size: config.font_size,
            monitor: Monitor::start(cc.egui_ctx.clone(), settings.done_sound),
            config,
            settings,
            ws: Workspace::default(),
            mode: VoiceMode::Dictation,
            pipeline: Pipeline::new(),
            status: Status::default(),
            show_settings: false,
            loader: LoaderPanel::default(),
            show_loader: false,
            pending_commands: VecDeque::new(),
            library_notice: None,
            search_open: false,
            search_focus: false,
            search_note: None,
            renaming: None,
            show_help: false,
            confirm_quit: false,
            quit_confirmed: false,
        };
        if let Some(pane) = app.spawn_pane(&cc.egui_ctx) {
            app.ws.add_tab(pane);
        }
        app
    }

    // MARK: 窗格

    fn spawn_pane(&mut self, ctx: &egui::Context) -> Option<Pane> {
        // 初始尺寸随便给,首帧布局时会按实际区域 resize
        let size = GridSize { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
        match TerminalSession::spawn(ctx, size, self.config.scrollback, &self.settings, self.ws.next_pane_id()) {
            Ok(session) => {
                self.monitor.watch(&session.shared);
                Some(Pane { session, input: InputState::default(), search: SearchState::default() })
            }
            Err(e) => {
                self.set_status(format!("无法启动 shell:{e:#}"), true);
                None
            }
        }
    }

    fn new_tab(&mut self, ctx: &egui::Context) {
        if let Some(pane) = self.spawn_pane(ctx) {
            self.ws.add_tab(pane);
        }
    }

    fn split(&mut self, ctx: &egui::Context, axis: Axis) {
        if let Some(pane) = self.spawn_pane(ctx) {
            self.ws.split_active(pane, axis);
        }
    }

    /// 关掉的窗格在 drop 时写完记录、挂断 shell。最后一个也关了就退出(这是明确的操作,不再确认)
    fn close_active(&mut self, ctx: &egui::Context) {
        drop(self.ws.close_active());
        if self.ws.tabs.is_empty() {
            self.quit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn active_session(&self) -> Option<&TerminalSession> {
        self.ws.active_pane().map(|p| &p.session)
    }

    fn active_cwd(&self) -> Option<PathBuf> {
        self.active_session().and_then(|s| s.shared.cwd())
    }

    pub(crate) fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Status { text: text.into(), is_error, at: Some(Instant::now()) };
    }

    pub(crate) fn reload_config(&mut self, ctx: &egui::Context) {
        self.config = effective_config(&self.settings);
        widgets::apply_visuals(ctx, &self.config.theme());
        if let Err(e) = self.settings.save() {
            self.set_status(format!("设置保存失败:{e}"), true);
        }
    }

    /// 标签名:手动改的优先;否则各分屏的目录名用 - 连起来
    fn tab_title(&self, index: usize) -> String {
        let tab = &self.ws.tabs[index];
        if let Some(t) = tab.custom_title.as_ref().filter(|t| !t.is_empty()) {
            return t.clone();
        }
        let names: Vec<String> =
            tab.panes().iter().filter_map(|id| self.ws.panes.get(id)).map(|p| p.session.display_name()).collect();
        if names.is_empty() { "shell".into() } else { names.join("-") }
    }

    // MARK: 快捷键

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use Action::*;
        if NewTab.consume(ctx) {
            self.new_tab(ctx);
        }
        if Close.consume(ctx) {
            self.close_active(ctx);
        }
        if SplitRight.consume(ctx) {
            self.split(ctx, Axis::Horizontal);
        }
        if SplitDown.consume(ctx) {
            self.split(ctx, Axis::Vertical);
        }
        if NextTab.consume(ctx) {
            self.ws.cycle_tab(1);
        }
        if PrevTab.consume(ctx) {
            self.ws.cycle_tab(-1);
        }
        for n in 1..=8u8 {
            if Tab(n).consume(ctx) {
                self.ws.select_tab(n as usize - 1);
            }
            if MoveToTab(n).consume(ctx) {
                self.ws.move_active_to_tab(n as usize - 1);
            }
        }
        if LastTab.consume(ctx) {
            self.ws.select_tab(self.ws.tabs.len().saturating_sub(1));
        }
        for (action, dir) in [(FocusLeft, Dir::Left), (FocusRight, Dir::Right), (FocusUp, Dir::Up), (FocusDown, Dir::Down)] {
            if action.consume(ctx) {
                self.ws.focus_dir(dir);
            }
        }
        if MoveToNewTab.consume(ctx) {
            self.ws.move_active_to_new_tab();
        }
        if MoveToPrevTab.consume(ctx) {
            self.ws.move_active_adjacent(-1);
        }
        if MoveToNextTab.consume(ctx) {
            self.ws.move_active_adjacent(1);
        }
        if FontBigger.consume(ctx) {
            self.font_size = (self.font_size + 1.0).min(FONT_MAX);
        }
        if FontSmaller.consume(ctx) {
            self.font_size = (self.font_size - 1.0).max(FONT_MIN);
        }
        if FontReset.consume(ctx) {
            self.font_size = self.config.font_size;
        }
        if ClearScreen.consume(ctx) {
            if let Some(s) = self.active_session() {
                s.clear_screen();
            }
        }
        if Find.consume(ctx) {
            self.search_open = true;
            self.search_focus = true;
        }
        if SelectAll.consume(ctx) {
            if let Some(s) = self.active_session() {
                view::select_all(s);
            }
        }
        if ToggleRecording.consume(ctx) {
            let action = if self.pipeline.is_recording() { PttAction::StopRecording } else { PttAction::StartRecording };
            self.on_ptt(action);
        }
        if ToggleMode.consume(ctx) {
            self.mode = match self.mode {
                VoiceMode::Dictation => VoiceMode::NaturalLanguage,
                VoiceMode::NaturalLanguage => VoiceMode::Dictation,
            };
        }
        if Loader.consume(ctx) {
            self.toggle_loader();
        }
        if Settings.consume(ctx) {
            self.show_settings = !self.show_settings;
        }
        if Help.consume(ctx) {
            self.show_help = !self.show_help;
        }
    }

    // MARK: 每帧的后台事件

    fn pump(&mut self, ctx: &egui::Context) {
        let theme = self.config.theme();
        let mut clipboard = None;
        for pane in self.ws.panes.values_mut() {
            pane.session.pump_events(&theme, &mut clipboard);
        }
        if let Some(text) = clipboard {
            ctx.copy_text(text);
        }
        while self.pending_commands.front().is_some_and(|(at, _)| *at <= Instant::now()) {
            let (_, cmd) = self.pending_commands.pop_front().unwrap();
            if let Some(s) = self.active_session() {
                // 用 \r(回车键)而不是 \n:claude 这类 TUI 在 raw 模式下认的是 \r
                s.write(format!("{cmd}\r").into_bytes());
            }
        }
        if let Some((at, _)) = self.pending_commands.front() {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }

        // shell 退出的窗格自动关闭;全部关完就退出
        if !self.ws.retain(|p| !p.session.exited).is_empty() && self.ws.tabs.is_empty() {
            self.quit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        while let Ok(event) = self.pipeline.rx.try_recv() {
            match event {
                PipelineEvent::Stage(s) => self.set_status(s, false),
                PipelineEvent::Insert { text, explanation } => {
                    self.pipeline.finished();
                    if let Some(s) = self.active_session() {
                        s.type_text(&text);
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
                Ok(()) => self.set_status("正在听…松开空格(或再按一次录音快捷键)结束", false),
                Err(e) => self.set_status(e, true),
            },
            PttAction::StopRecording => {
                let cwd = self
                    .active_session()
                    .and_then(|s| s.current_directory())
                    .map(|p| p.to_string_lossy().into_owned());
                self.pipeline.stop(&self.config, self.mode, cwd);
            }
            _ => {}
        }
    }

    // MARK: 标签栏

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mut close = None;
            let mut select = None;
            for i in 0..self.ws.tabs.len() {
                let statuses: Vec<_> = self.ws.tabs[i]
                    .panes()
                    .iter()
                    .filter_map(|id| self.ws.panes.get(id))
                    .map(|p| p.session.shared.status())
                    .collect();
                widgets::status_dots(ui, &statuses);

                if let Some((ri, draft)) = self.renaming.as_mut().filter(|(ri, _)| *ri == i) {
                    let edit = ui.add(egui::TextEdit::singleline(draft).desired_width(120.0));
                    edit.request_focus();
                    let (enter, esc) = ui.input(|inp| (inp.key_pressed(Key::Enter), inp.key_pressed(Key::Escape)));
                    if esc {
                        self.renaming = None;
                    } else if enter || edit.lost_focus() {
                        let title = draft.trim().to_string();
                        let idx = *ri;
                        self.ws.tabs[idx].custom_title = (!title.is_empty()).then_some(title);
                        self.renaming = None;
                    }
                    continue;
                }

                let label = format!("{} {}", i + 1, widgets::truncate(&self.tab_title(i), 28));
                let hover = self.ws.tabs[i]
                    .panes()
                    .iter()
                    .filter_map(|id| self.ws.panes.get(id))
                    .map(|p| p.session.title.clone())
                    .collect::<Vec<_>>()
                    .join(" | ");
                let r = ui.selectable_label(i == self.ws.active_tab, label).on_hover_text(hover);
                if r.double_clicked() {
                    self.renaming = Some((i, self.ws.tabs[i].custom_title.clone().unwrap_or_default()));
                } else if r.clicked() {
                    select = Some(i);
                }
                if r.middle_clicked() {
                    close = Some(i);
                }
            }
            if ui.button("＋").on_hover_text(format!("新标签页 ({})", Action::NewTab.label())).clicked() {
                self.new_tab(ui.ctx());
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
        });
    }

    // MARK: 搜索栏

    fn search_bar(&mut self, ui: &mut egui::Ui) {
        let Some(pane) = self.ws.active_pane_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label("🔍");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut pane.search.query)
                    .hint_text("搜索历史(回车更早,⇧回车更新,Esc 关闭)")
                    .desired_width(320.0),
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
            if ui.small_button("▲").on_hover_text("更早").clicked() {
                step = Some(false);
            }
            if ui.small_button("▼").on_hover_text("更新").clicked() {
                step = Some(true);
            }
            if let Some(forward) = step {
                self.search_note = match pane.search.step(&pane.session, forward) {
                    SearchResult::Found | SearchResult::Empty => None,
                    SearchResult::NotFound => Some("没有找到".into()),
                };
            }
            if let Some(n) = &self.search_note {
                ui.colored_label(ERROR, n);
            }
            if esc || ui.small_button("✕").clicked() {
                close = true;
            }
        });
        if close {
            self.search_open = false;
            self.search_note = None;
            pane.session.term.lock().selection = None;
        }
    }

    // MARK: 状态栏

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for mode in [VoiceMode::Dictation, VoiceMode::NaturalLanguage] {
                if ui
                    .selectable_label(self.mode == mode, mode.label())
                    .on_hover_text(format!("切换:{}", Action::ToggleMode.label()))
                    .clicked()
                {
                    self.mode = mode;
                }
            }
            ui.separator();

            if self.pipeline.is_recording() {
                let level = (self.pipeline.level() * 8.0).clamp(0.0, 1.0);
                if ui.button(RichText::new("● REC").color(Color32::from_rgb(230, 70, 70))).on_hover_text("点击停止").clicked() {
                    self.on_ptt(PttAction::StopRecording);
                }
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
                let r = ui.add(egui::Label::new(RichText::new(format!("🎤 按住空格说话 · {engine}")).weak()).sense(egui::Sense::click()));
                if r.on_hover_text(format!("点击开始录音({})", Action::ToggleRecording.label())).clicked() {
                    self.on_ptt(PttAction::StartRecording);
                }
            }
            ui.separator();

            // 状态文字 15 秒后淡出(错误保留到下一次操作)
            let fresh = self.status.at.is_some_and(|t| t.elapsed() < Duration::from_secs(15));
            if fresh || self.status.is_error {
                let color = if self.status.is_error { ERROR } else { ui.visuals().text_color() };
                ui.label(RichText::new(&self.status.text).color(color));
            }

            let usage = self.active_session().and_then(|s| s.shared.usage());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙").on_hover_text(format!("设置({})", Action::Settings.label())).clicked() {
                    self.show_settings = !self.show_settings;
                }
                if ui.button("⌨").on_hover_text(format!("快捷键({})", Action::Help.label())).clicked() {
                    self.show_help = !self.show_help;
                }
                if ui
                    .button("🧩")
                    .on_hover_text(format!(
                        "加载器({}):扫描设置里添加的文件夹中的插件、技能、Agent,勾选的会加载进当前目录",
                        Action::Loader.label()
                    ))
                    .clicked()
                {
                    self.toggle_loader();
                }
                self.projects_menu(ui);
                if let Some(usage) = usage {
                    widgets::claude_meter(ui, &usage);
                }
            });
        });
    }

    // MARK: 🧩 与常用项目

    fn toggle_loader(&mut self) {
        self.show_loader = !self.show_loader;
        if self.show_loader {
            self.loader.open(self.active_cwd().as_deref(), &self.config.skill_library_dirs);
        }
    }

    fn loader_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_loader;
        let cwd = self.active_cwd();
        let dirs = self.config.skill_library_dirs.clone();
        let mut collapsed = self.settings.loader_groups_collapsed.clone();
        let mut output = None;
        egui::Window::new("🧩 加载器").open(&mut open).resizable(false).default_width(380.0).show(ctx, |ui| {
            ui.set_width(380.0);
            output = Some(self.loader.show(ui, cwd.as_deref(), &dirs, &mut collapsed));
        });
        self.show_loader = open;
        let Some(out) = output else { return };
        if out.collapsed_changed {
            self.settings.loader_groups_collapsed = collapsed;
            let _ = self.settings.save();
        }
        if out.close {
            self.show_loader = false;
        }
        let now = Instant::now();
        for (i, cmd) in out.reloads.into_iter().enumerate() {
            self.pending_commands.push_back((now + Duration::from_secs(2 * i as u64), cmd.to_string()));
        }
    }

    /// 常用项目:cd 进入目录并启动 claude(发送到当前窗格)
    fn projects_menu(&mut self, ui: &mut egui::Ui) {
        let mut launch = None;
        let mut remove = None;
        let mut add = false;
        ui.menu_button("✨", |ui| {
            let mut dirs = self.settings.claude_dirs.clone();
            dirs.sort_by_key(|d| (widgets::dir_name(d).to_lowercase(), d.clone()));
            if dirs.is_empty() {
                ui.label(RichText::new("还没有保存的目录 —— 用「添加目录…」").small().weak());
            }
            for dir in dirs {
                ui.horizontal(|ui| {
                    if ui.button(format!("📁 {}", widgets::dir_name(&dir))).on_hover_text(dir.display().to_string()).clicked() {
                        launch = Some(dir.clone());
                        ui.close();
                    }
                    if ui.small_button("🗑").on_hover_text("移除").clicked() {
                        remove = Some(dir.clone());
                    }
                });
            }
            ui.separator();
            if ui.button("＋ 添加目录…").clicked() {
                add = true;
                ui.close();
            }
        })
        .response
        .on_hover_text("常用项目:cd 进入目录并启动 Claude(发送到当前窗格)");

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

    fn help_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("快捷键").open(&mut self.show_help).resizable(false).show(ctx, |ui| {
            for (title, rows) in shortcuts::help_sections() {
                ui.label(RichText::new(title).strong());
                egui::Grid::new(title).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    for (keys, desc) in rows {
                        ui.label(RichText::new(keys).monospace());
                        ui.label(desc);
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
            }
        });
    }

    fn quit_dialog(&mut self, ctx: &egui::Context) {
        egui::Modal::new(egui::Id::new("confirm-quit")).show(ctx, |ui| {
            ui.label(RichText::new("退出 VoiceGhostty?").strong());
            ui.label("所有标签页里的 shell(以及其中运行的程序)都会被挂断。");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("退出").clicked() {
                    self.quit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("取消").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.confirm_quit = false;
                }
            });
        });
    }

    // MARK: 终端区

    fn terminals(&mut self, ui: &mut egui::Ui) {
        let Some(tab) = self.ws.active_tab() else { return };
        let frames = tab.root.frames(ui.available_rect_before_wrap(), SPLIT_GAP);
        let (active, split) = (tab.active, tab.is_split());
        let window_focused = ui.input(|i| i.focused);
        let theme = self.config.theme();
        // 键盘焦点归终端,除非搜索框或改名框在用
        let terminal_owns_keyboard = !self.search_open && self.renaming.is_none();

        let mut clicked = None;
        let mut actions = vec![];
        for (id, rect) in frames {
            let Some(pane) = self.ws.panes.get_mut(&id) else { continue };
            let is_active = id == active;
            let opts = ViewOptions {
                theme: &theme,
                font_size: self.font_size,
                hold: Duration::from_millis(self.config.voice.hold_ms),
                ptt_enabled: self.config.voice.engine != VoiceEngineKind::Off && !self.pipeline.is_busy(),
                grab_focus: is_active && terminal_owns_keyboard,
            };
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("pane", id)));
            let out = view::show(&mut child, &mut pane.session, &mut pane.input, &opts);
            if out.clicked {
                clicked = Some(id);
            }
            actions.extend(out.actions);
            // 分屏时给当前窗格描一圈,看得出语音和输入会去哪
            if split && is_active {
                let stroke = egui::Stroke::new(1.0, ui.visuals().selection.stroke.color.gamma_multiply(0.7));
                ui.painter().rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
            }
            // 你正盯着的那个窗格保持灰色;分屏时每个窗格都照常推进(两边都能看到黄/绿)。
            // 窗口到后台后它恰恰是最值得报告的那个,于是也照常推进
            pane.session.shared.neutral.store(window_focused && is_active && !split, Ordering::Relaxed);
        }
        ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        if let Some(id) = clicked {
            self.ws.focus_pane(id);
        }
        for action in actions {
            self.on_ptt(action);
        }
    }
}

impl eframe::App for VoiceGhosttyApp {
    /// 退出时把各窗格屏幕上剩下的行写进记录(窗格 drop 时完成),并挂断 shell;再释放 whisper 模型
    fn on_exit(&mut self) {
        self.ws.panes.clear();
        vg_voice::shutdown();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 关窗口先确认:会挂断所有 shell
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_quit = true;
        }

        self.handle_shortcuts(&ctx);
        self.pump(&ctx);
        // 当前标签页之外的窗格:不是你正看着的,照常推进状态灯
        let visible: Vec<_> = self.ws.active_tab().map(|t| t.panes()).unwrap_or_default();
        for (id, pane) in &self.ws.panes {
            if !visible.contains(id) {
                pane.session.shared.neutral.store(false, Ordering::Relaxed);
            }
        }

        if let Some(s) = self.active_session() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — VoiceGhostty", s.title)));
        }

        egui::Panel::top("tabs").show(ui, |ui| self.tab_bar(ui));
        if self.search_open {
            egui::Panel::top("search").show(ui, |ui| self.search_bar(ui));
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        let bg = self.config.theme().background;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(bg.0, bg.1, bg.2)).inner_margin(4.0))
            .show(ui, |ui| self.terminals(ui));

        if self.show_settings {
            self.settings_window(&ctx);
        }
        if self.show_loader {
            self.loader_window(&ctx);
        }
        if self.show_help {
            self.help_window(&ctx);
        }
        if self.confirm_quit {
            self.quit_dialog(&ctx);
        }
    }
}
