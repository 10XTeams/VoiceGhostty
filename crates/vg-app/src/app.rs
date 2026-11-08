//! 主窗口:标签栏 + 分屏终端 + 搜索栏 + 状态栏(语音模式、引擎、录音电平、结果说明、claude 用量)
//! 以及设置 / 🧩 加载器 / 快捷键帮助 / 退出确认这几个浮窗。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};


use vg_core::nl2command::TerminalContext;
use vg_core::voice_commands::VoiceCommand;
use vg_core::{Config, Settings, VoiceEngineKind};

use crate::hotkey::{GlobalHotkey, HotkeyEvent};
use crate::loader_panel::LoaderPanel;
use crate::monitor::Monitor;
use crate::pipeline::{Pipeline, PipelineEvent, VoiceMode};
use crate::ptt::PttAction;
use crate::shortcuts::Action;
use crate::split::{Axis, Dir};
use crate::terminal::{self, search::SearchState};
use crate::terminal::session::{GridSize, TerminalSession};
use crate::terminal::view::{self, InputState, ViewOptions};
use crate::style;
use crate::workspace::{PaneId, Workspace};

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
/// 自然语言模式带给 LLM 的屏幕行数
const SCREEN_CONTEXT_LINES: usize = 40;
const FONT_MIN: f32 = 8.0;
const FONT_MAX: f32 = 48.0;

/// 语音结果往哪里落:终端光标处,或者开始录音时持有焦点的那个文本框(搜索、改名、设置里的输入框)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceTarget {
    Terminal,
    Field(egui::Id),
}

/// 一次语音上屏的记录(撤销用)
#[derive(Clone, Copy, Debug)]
struct VoiceInsert {
    pane: PaneId,
    chars: usize,
    typed_seq: u64,
}

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
    /// 实时字幕:录音期间到目前为止识别出的内容,松开后保留到最终结果出来
    pub(crate) caption: Option<String>,
    /// 上一次语音上屏:落在哪个窗格、多少个字符、当时该窗格的敲键计数。撤销要三者都对得上
    last_voice_insert: Option<VoiceInsert>,
    /// 这一帧终端控件是否持有键盘焦点,以及 egui 里当前持有焦点的控件(每帧开头采样)
    terminal_focused: bool,
    focused_widget: Option<egui::Id>,
    /// 这次录音开始时定下的落点
    voice_target: VoiceTarget,
    /// 全局热键(配置了且注册成功时)与它按下的时刻:松开时按住够久就停止,否则当作开关
    global_hotkey: Option<GlobalHotkey>,
    global_hotkey_pressed: Option<Instant>,
    /// 注册失败的原因,设置页显示
    pub(crate) global_hotkey_error: Option<String>,
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
            caption: None,
            last_voice_insert: None,
            terminal_focused: true,
            focused_widget: None,
            voice_target: VoiceTarget::Terminal,
            global_hotkey: None,
            global_hotkey_pressed: None,
            global_hotkey_error: None,
            #[cfg(test)]
            preview_voice: None,
        };
        app.sync_global_hotkey(&cc.egui_ctx);
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
        self.sync_global_hotkey(ctx);
        if let Err(e) = self.settings.save() {
            self.set_status(format!("设置保存失败:{e}"), true);
        }
    }

    /// 让已注册的全局热键跟配置一致:配置变了就注销重注册,清空就注销
    fn sync_global_hotkey(&mut self, ctx: &egui::Context) {
        let want = self.config.voice.global_hotkey.trim().to_string();
        if self.global_hotkey.as_ref().map(|h| h.spec.as_str()) == Some(want.as_str()) {
            return;
        }
        self.global_hotkey = None; // 先注销旧的
        self.global_hotkey_error = None;
        if want.is_empty() {
            return;
        }
        match GlobalHotkey::register(&want, ctx) {
            Ok(h) => self.global_hotkey = h,
            Err(e) => {
                log::warn!("global hotkey: {e}");
                self.global_hotkey_error = Some(e);
            }
        }
    }

    /// 说话键(配置里的按键名,认不出来就回到空格)
    pub(crate) fn ptt_key(&self) -> egui::Key {
        egui::Key::from_name(&self.config.voice.ptt_key).unwrap_or(egui::Key::Space)
    }

    /// 已注册的全局热键的标签(`⌃⌥Space`),没配或注册失败则 None
    pub(crate) fn global_hotkey_label(&self) -> Option<String> {
        self.global_hotkey.as_ref().map(|h| crate::hotkey::pretty(&h.spec))
    }

    /// 说话键给人看的名字
    pub(crate) fn ptt_key_label(&self) -> String {
        match self.ptt_key() {
            egui::Key::Space => "空格".to_string(),
            k => k.name().to_string(),
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
        if UndoVoice.consume(ctx) {
            if self.undo_voice() {
                self.set_status("已撤销上一次语音输入", false);
            } else {
                self.set_status("没有可撤销的语音输入", true);
            }
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
        if Sidebar.consume(ctx) {
            self.toggle_sidebar();
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

        // 全局热键:按下开始录音;松开时按住够久就停止,点一下则保持录音、再按一下停止
        let mut hotkey_events = Vec::new();
        if let Some(h) = &self.global_hotkey {
            while let Ok(ev) = h.rx.try_recv() {
                hotkey_events.push(ev);
            }
        }
        for ev in hotkey_events {
            let hold = Duration::from_millis(self.config.voice.hold_ms);
            match ev {
                HotkeyEvent::Pressed => {
                    if self.pipeline.is_recording() {
                        self.global_hotkey_pressed = None;
                        self.on_ptt(PttAction::StopRecording);
                    } else if !self.pipeline.is_busy() {
                        self.global_hotkey_pressed = Some(Instant::now());
                        self.on_ptt(PttAction::StartRecording);
                    }
                }
                HotkeyEvent::Released => {
                    if let Some(at) = self.global_hotkey_pressed.take() {
                        if at.elapsed() >= hold && self.pipeline.is_recording() {
                            self.on_ptt(PttAction::StopRecording);
                        }
                    }
                }
            }
        }

        while let Ok(event) = self.pipeline.rx.try_recv() {
            match event {
                PipelineEvent::Stage(s) => self.set_status(s, false),
                PipelineEvent::Partial(text) => {
                    // 松开之后字幕线程可能还会送来最后一轮,只在录音 / 识别期间接受
                    if self.pipeline.is_recording() || self.pipeline.is_busy() {
                        self.caption = Some(text);
                    }
                }
                PipelineEvent::Insert { text, explanation } => {
                    self.pipeline.finished();
                    self.caption = None;
                    match self.voice_target {
                        // 开始录音时焦点在某个文本框里:把文字作为键盘输入事件交给它。
                        // pump 在控件绘制之前跑,这一帧的输入事件它正好能收到;先把焦点还给它
                        // (点状态栏胶囊停止录音会让输入框失焦)
                        VoiceTarget::Field(id) => {
                            ctx.memory_mut(|m| m.request_focus(id));
                            ctx.input_mut(|i| i.events.push(egui::Event::Text(text.clone())));
                            self.last_voice_insert = None;
                        }
                        VoiceTarget::Terminal => {
                            if let Some(pane) = self.ws.active_pane() {
                                pane.session.type_text(&text);
                                self.last_voice_insert = Some(VoiceInsert {
                                    pane: self.ws.active_pane_id().unwrap_or_default(),
                                    chars: text.chars().count(),
                                    typed_seq: pane.input.typed_seq,
                                });
                            }
                        }
                    }
                    let note = explanation.unwrap_or_else(|| format!("已上屏:{text}"));
                    self.set_status(note, false);
                }
                PipelineEvent::Action(cmd) => {
                    self.pipeline.finished();
                    self.caption = None;
                    self.run_voice_command(ctx, cmd);
                }
                PipelineEvent::Nothing => {
                    self.pipeline.finished();
                    self.caption = None;
                    self.set_status("没听到声音", false);
                }
                PipelineEvent::Error(e) => {
                    self.pipeline.finished();
                    self.caption = None;
                    self.set_status(e, true);
                }
            }
        }
    }

    fn on_ptt(&mut self, action: PttAction) {
        match action {
            PttAction::StartRecording => match self.pipeline.start(&self.config, self.hotwords()) {
                Ok(()) => {
                    // 焦点不在终端而在某个文本框里(搜索、改名、设置):这次说的话归它
                    self.voice_target = match self.focused_widget {
                        Some(id) if !self.terminal_focused => VoiceTarget::Field(id),
                        _ => VoiceTarget::Terminal,
                    };
                    self.rec_started = Some(Instant::now());
                    self.levels = std::iter::repeat_n(0.0, chrome::WAVE_BARS).collect();
                    self.status = Status::default();
                    self.caption = None;
                }
                Err(e) => self.set_status(e, true),
            },
            PttAction::StopRecording => {
                let context = self.terminal_context();
                self.pipeline.stop(&self.config, self.mode, context);
            }
            _ => {}
        }
    }

    /// 执行一条语音动作命令(与对应快捷键走同一套方法)
    fn run_voice_command(&mut self, ctx: &egui::Context, cmd: VoiceCommand) {
        match cmd {
            VoiceCommand::ClearScreen => {
                if let Some(s) = self.active_session() {
                    s.clear_screen();
                }
            }
            VoiceCommand::NewTab => self.new_tab(ctx),
            VoiceCommand::CloseActive => self.close_active(ctx),
            VoiceCommand::NextTab => self.ws.cycle_tab(1),
            VoiceCommand::PrevTab => self.ws.cycle_tab(-1),
            VoiceCommand::Tab(n) => {
                if n as usize > self.ws.tabs.len() {
                    self.set_status(format!("只有 {} 个标签页", self.ws.tabs.len()), true);
                    return;
                }
                self.ws.select_tab(n as usize - 1);
            }
            VoiceCommand::LastTab => self.ws.select_tab(self.ws.tabs.len().saturating_sub(1)),
            VoiceCommand::SplitRight => self.split(ctx, Axis::Horizontal),
            VoiceCommand::SplitDown => self.split(ctx, Axis::Vertical),
            VoiceCommand::Find => {
                self.search_open = true;
                self.search_focus = true;
            }
            VoiceCommand::Dictation => self.mode = VoiceMode::Dictation,
            VoiceCommand::NaturalLanguage => self.mode = VoiceMode::NaturalLanguage,
            VoiceCommand::UndoVoice => {
                if !self.undo_voice() {
                    self.set_status("没有可撤销的语音输入", true);
                    return;
                }
            }
        }
        self.set_status(format!("已执行:{}", cmd.label()), false);
    }

    /// 撤销上一次语音上屏:往那个窗格发同样多个退格。只在该窗格此后没再敲过键、且它仍然存在时才做,
    /// 否则退格会吃掉你自己打的字。一次上屏只能撤一次
    fn undo_voice(&mut self) -> bool {
        let Some(rec) = self.last_voice_insert else { return false };
        let Some(pane) = self.ws.panes.get(&rec.pane) else {
            self.last_voice_insert = None;
            return false;
        };
        if pane.input.typed_seq != rec.typed_seq || rec.chars == 0 {
            self.last_voice_insert = None;
            return false;
        }
        // 语音文本不含换行,逐字符退格即可;宽字符在 zsh / readline 里也是一个退格一个字
        pane.session.write(vec![0x7f; rec.chars]);
        self.last_voice_insert = None;
        true
    }

    /// 这次录音的热词:手动列表 + 活动窗格目录里的文件名与分支名
    fn hotwords(&self) -> Vec<String> {
        let cwd = self.active_session().and_then(|s| s.current_directory()).or_else(|| self.active_cwd());
        vg_core::hotwords::collect(cwd.as_deref(), &self.config.voice.hotwords, self.config.voice.hotwords_auto)
    }

    /// 活动窗格的终端现场:工作目录,以及(开着 `llm-screen-context` 时)屏幕尾部与上一条命令
    fn terminal_context(&self) -> TerminalContext {
        let Some(session) = self.active_session() else { return TerminalContext::default() };
        let mut ctx = TerminalContext::cwd(session.current_directory().map(|p| p.to_string_lossy().into_owned()));
        if self.config.llm_screen_context {
            ctx.screen_tail = Some(session.screen_tail(SCREEN_CONTEXT_LINES));
            ctx.last_command = session.shared.last_command();
        }
        ctx
    }

    pub(crate) fn toggle_sidebar(&mut self) {
        self.settings.show_sidebar = !self.settings.show_sidebar;
        let _ = self.settings.save();
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

        let ptt_key = self.ptt_key();
        let mut clicked = None;
        let mut actions = vec![];
        let mut any_focused = false;
        for (id, rect) in frames {
            let Some(pane) = self.ws.panes.get_mut(&id) else { continue };
            let is_active = id == active;
            let opts = ViewOptions {
                theme: &theme,
                font_size: self.font_size,
                hold: Duration::from_millis(self.config.voice.hold_ms),
                ptt_enabled: self.config.voice.engine != VoiceEngineKind::Off && !self.pipeline.is_busy(),
                ptt_key,
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
            if let Some(url) = out.open_url {
                terminal::links::open(&url);
            }
            any_focused |= out.focused;
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
        self.terminal_focused = any_focused;
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

        // 在任何控件改动焦点之前记下谁持有焦点:开始录音时据此决定结果落到终端还是文本框
        self.focused_widget = ctx.memory(|m| m.focused());
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
        if self.settings.show_sidebar {
            egui::Panel::left("panes")
                .exact_size(style::SIDEBAR_W)
                .frame(egui::Frame::NONE.fill(p.chrome).inner_margin(egui::Margin { left: 8, right: 6, top: 6, bottom: 6 }))
                .show(ui, |ui| self.sidebar(ui));
        }

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
