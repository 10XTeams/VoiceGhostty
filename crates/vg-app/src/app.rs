//! 主窗口:标签栏 + 分屏终端 + 搜索栏 + 状态栏(语音模式、引擎、录音电平、结果说明、claude 用量)
//! 以及设置 / 🧩 加载器 / 快捷键帮助 / 退出确认这几个浮窗。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};


use vg_core::{Config, Settings, VoiceEngineKind};

use crate::loader_panel::LoaderPanel;
use crate::monitor::Monitor;
use crate::pipeline::{Pipeline, PipelineEvent, VoiceMode};
use crate::ptt::PttAction;
use crate::shortcuts::Action;
use crate::split::{Axis, Dir};
use crate::terminal::search::SearchState;
use crate::terminal::session::{GridSize, TerminalSession};
use crate::terminal::view::{self, InputState, ViewOptions};
use crate::style;
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

/// 分屏之间的间隙(露出底下的分隔线颜色)
const SPLIT_GAP: f32 = 1.0;
const FONT_MIN: f32 = 8.0;
const FONT_MAX: f32 = 48.0;

/// 状态栏语音胶囊的几种样子
#[derive(Clone, Debug)]
pub(crate) enum VoiceUi {
    /// 可以说话;带引擎名
    Idle(String),
    /// 没有可用的引擎;带原因
    Unconfigured(String),
    Off,
    Recording { secs: f32, levels: Vec<f32> },
    /// 识别 / 纠错 / 生成命令中;带阶段名
    Working(String),
}

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
    /// macOS:菜单「退出」(⌘Q)是否已改成走关闭窗口的确认流程;菜单可能晚于首帧建好,没改到就下一帧再试
    quit_menu_patched: bool,
    /// 录音开始的时刻与最近的电平(状态栏波形,最新的在最后)
    rec_started: Option<Instant>,
    levels: VecDeque<f32>,
    level_sampled: Option<Instant>,
    #[cfg(test)]
    preview_voice: Option<VoiceUi>,
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
        style::apply(&cc.egui_ctx, &config.theme());

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
            quit_menu_patched: !cfg!(target_os = "macos"),
            rec_started: None,
            levels: VecDeque::new(),
            level_sampled: None,
            #[cfg(test)]
            preview_voice: None,
        };
        let first_cwd = app.settings.default_dir.clone();
        if let Some(pane) = app.spawn_pane(&cc.egui_ctx, first_cwd) {
            app.ws.add_tab(pane);
        }
        app
    }

    // MARK: 窗格

    /// `cwd`:新 shell 的起始目录,None 为家目录
    fn spawn_pane(&mut self, ctx: &egui::Context, cwd: Option<PathBuf>) -> Option<Pane> {
        // 初始尺寸随便给,首帧布局时会按实际区域 resize
        let size = GridSize { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
        match TerminalSession::spawn(ctx, size, self.config.scrollback, &self.settings, self.ws.next_pane_id(), cwd) {
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

    /// 新标签页从设置里的默认文件夹开始(没设就是家目录)
    fn new_tab(&mut self, ctx: &egui::Context) {
        if let Some(pane) = self.spawn_pane(ctx, self.settings.default_dir.clone()) {
            self.ws.add_tab(pane);
        }
    }

    /// 分屏出来的新格接着在原窗格的目录里开 shell。直接问操作系统拿最新的目录,
    /// 比轮询缓存的 cwd 新鲜(刚 cd 完立刻 ⌘D 也对)。
    fn split(&mut self, ctx: &egui::Context, axis: Axis) {
        let cwd = self.active_session().and_then(|s| s.current_directory()).or_else(|| self.active_cwd());
        if let Some(pane) = self.spawn_pane(ctx, cwd) {
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
        style::apply(ctx, &self.config.theme());
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

        // 录音电平:约每 40ms 取一次,推进状态栏波形
        if self.pipeline.is_recording() {
            let due = self.level_sampled.is_none_or(|t| t.elapsed() >= Duration::from_millis(40));
            if due {
                self.levels.push_back(self.pipeline.level());
                while self.levels.len() > chrome::WAVE_BARS {
                    self.levels.pop_front();
                }
                self.level_sampled = Some(Instant::now());
            }
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
                Ok(()) => {
                    self.rec_started = Some(Instant::now());
                    self.levels = std::iter::repeat_n(0.0, chrome::WAVE_BARS).collect();
                    self.status = Status::default();
                }
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
        crate::kit::window("加载器").anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(420.0);
            if crate::kit::window_header(ui, "加载器", Some("把技能、Agent、插件加载进当前目录")) {
                open = false;
            }
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

    // MARK: 终端区

    fn terminals(&mut self, ui: &mut egui::Ui) {
        let Some(tab) = self.ws.active_tab() else { return };
        let frames = tab.root.frames(ui.available_rect_before_wrap(), SPLIT_GAP);
        let (active, split) = (tab.active, tab.is_split());
        let window_focused = ui.input(|i| i.focused);
        let theme = self.config.theme();
        let p = style::palette(ui.ctx());
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
            // 底色铺满整个窗格,文字往里缩一点:贴边的字不好读
            ui.painter().rect_filled(rect, 0.0, p.term);
            let inner = rect.shrink2(style::TERM_PAD);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(("pane", id)));
            let out = view::show(&mut child, &mut pane.session, &mut pane.input, &opts);
            if out.clicked {
                clicked = Some(id);
            }
            actions.extend(out.actions);
            // 分屏时其余窗格压暗一层,看得出语音和输入会去哪
            if split && !is_active {
                ui.painter().rect_filled(rect, 0.0, style::alpha(p.chrome, 0.42));
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

        #[cfg(target_os = "macos")]
        if !self.quit_menu_patched {
            self.quit_menu_patched = crate::macos::redirect_quit_to_close();
        }

        // 关窗口先确认:会挂断所有 shell(macOS 上 ⌘Q 也走这里,见 macos.rs)
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

        let p = style::palette(&ctx);
        egui::Panel::top("tabs")
            .exact_size(style::BAR_H)
            .frame(egui::Frame::NONE.fill(p.chrome).inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 0 }))
            .show(ui, |ui| self.tab_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(style::STATUS_H)
            .frame(egui::Frame::NONE.fill(p.chrome).inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| self.status_bar(ui));

        // 底色是分隔线的颜色:分屏的缝隙露出来就是一条细线
        let central = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.border))
            .show(ui, |ui| self.terminals(ui));
        if self.search_open {
            self.search_box(&ctx, central.response.rect);
        }

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

#[path = "chrome.rs"]
mod chrome;

#[cfg(test)]
#[path = "shots.rs"]
mod shots;
