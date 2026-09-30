//! 一个终端会话 = 一个 PTY + 一个 shell + 一个 alacritty 终端状态机。
//! PTY 读写在 alacritty 自己的 IO 线程里跑;UI 线程只在绘制时短暂加锁读网格。
//! 状态灯、会话记录、claude 用量这些"每隔一会儿看一眼"的事放在 [`PaneShared`] 里,由后台监视线程推进
//! (见 `monitor.rs`),这样窗口在后台、不重绘时提示音也照样会响。

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::Rgb as AnsiRgb;

use vg_core::claude_usage::{ContextUsage, UsageWatcher};
use vg_core::status::{Observation, ShellMark, ShellMarkTracker};
use vg_core::{paths, shell, SessionStatus, Settings, StatusMachine, Theme};

use super::history::SessionLogger;
use super::proc::{self, PtyProbe};

/// PTY IO 线程写、监视线程读的原子信号(不加锁,值幂等,竞争无害)
pub struct Signals {
    epoch: Instant,
    last_output_ms: AtomicU64,
    output_seq: AtomicU64,
    bell: AtomicBool,
}

impl Signals {
    fn new() -> Signals {
        Signals {
            epoch: Instant::now(),
            last_output_ms: AtomicU64::new(0),
            output_seq: AtomicU64::new(0),
            bell: AtomicBool::new(false),
        }
    }

    fn note_output(&self) {
        let ms = self.epoch.elapsed().as_millis() as u64;
        self.last_output_ms.store(ms, Ordering::Relaxed);
        self.output_seq.fetch_add(1, Ordering::Relaxed);
    }
}

/// 把终端事件转回 UI 线程,并唤醒 egui 重绘
#[derive(Clone)]
pub struct EventProxy {
    tx: Sender<Event>,
    ctx: egui::Context,
    signals: Arc<Signals>,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            // 每读到一批 PTY 输出发一次
            Event::Wakeup => self.signals.note_output(),
            Event::Bell => self.signals.bell.store(true, Ordering::Relaxed),
            other => {
                let _ = self.tx.send(other);
            }
        }
        self.ctx.request_repaint();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u16,
    pub cell_h: u16,
}

impl GridSize {
    fn window_size(self) -> WindowSize {
        WindowSize {
            num_cols: self.cols,
            num_lines: self.rows,
            cell_width: self.cell_w,
            cell_height: self.cell_h,
        }
    }
}

#[derive(Default)]
struct StatusState {
    machine: StatusMachine,
    marks: ShellMarkTracker,
    seen_output_seq: u64,
}

/// 状态机"安静多久算做完"
const SILENCE: Duration = Duration::from_secs(2);
/// 会话记录每 10 次(约 5 秒)写一次;claude 用量每 4 次(约 2 秒)读一次
const HISTORY_EVERY: u64 = 10;
const USAGE_EVERY: u64 = 4;

/// 窗格里 UI 线程与监视线程共享的部分
pub struct PaneShared {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    signals: Arc<Signals>,
    status: Mutex<StatusState>,
    usage_watcher: Mutex<UsageWatcher>,
    usage: Mutex<Option<ContextUsage>>,
    logger: Mutex<Option<SessionLogger>>,
    cwd: Mutex<Option<PathBuf>>,
    probe: Option<PtyProbe>,
    shell_pid: Option<u32>,
    state_file: PathBuf,
    scrollback: AtomicUsize,
    /// UI 设置:这是你正盯着的那个窗格(窗口在前台),保持灰色,不拿自己的灯烦自己
    pub neutral: AtomicBool,
}

/// 一次监视 tick 的结果
#[derive(Default)]
pub struct TickResult {
    pub changed: bool,
    pub chime: bool,
}

impl PaneShared {
    pub fn status(&self) -> SessionStatus {
        self.status.lock().unwrap().machine.status
    }

    pub fn usage(&self) -> Option<ContextUsage> {
        self.usage.lock().unwrap().clone()
    }

    pub fn cwd(&self) -> Option<PathBuf> {
        self.cwd.lock().unwrap().clone()
    }

    /// 监视线程每 0.5 秒调一次
    pub fn tick(&self, n: u64) -> TickResult {
        let mut result = TickResult::default();
        {
            let mut guard = self.status.lock().unwrap();
            let st = &mut *guard;
            let mark = std::fs::read_to_string(&self.state_file).ok().and_then(|s| ShellMark::parse(&s));
            if let Some(mark) = mark {
                st.marks.apply(mark, &mut st.machine);
            }
            let seq = self.signals.output_seq.load(Ordering::Relaxed);
            if seq != st.seen_output_seq {
                st.seen_output_seq = seq;
                let ms = self.signals.last_output_ms.load(Ordering::Relaxed);
                st.machine.note_output(self.signals.epoch + Duration::from_millis(ms));
            }
            if self.signals.bell.swap(false, Ordering::Relaxed) {
                st.machine.note_bell();
            }
            if self.neutral.load(Ordering::Relaxed) {
                result.changed = st.machine.reset();
            } else {
                let interactive_app = self.probe.as_ref().and_then(PtyProbe::is_raw_mode) == Some(true);
                let (changed, chime) =
                    st.machine.refresh(Observation { now: Instant::now(), interactive_app }, SILENCE);
                result = TickResult { changed, chime };
            }
        }

        // 当前目录:问操作系统,不依赖 shell 集成。拿不到时保留上次的值,别让标签名闪回
        if let Some(dir) = self.shell_pid.and_then(proc::process_cwd) {
            let mut cwd = self.cwd.lock().unwrap();
            if cwd.as_ref() != Some(&dir) {
                if let Some(logger) = self.logger.lock().unwrap().as_mut() {
                    logger.current_directory = Some(dir.clone());
                }
                *cwd = Some(dir);
                result.changed = true;
            }
        }

        if n.is_multiple_of(HISTORY_EVERY) {
            self.flush_history(false);
        }
        if n.is_multiple_of(USAGE_EVERY) {
            let dir = self.probe.as_ref().and_then(PtyProbe::claude_working_directory);
            let next = self.usage_watcher.lock().unwrap().refresh(dir.as_deref());
            let mut usage = self.usage.lock().unwrap();
            if *usage != next {
                *usage = next;
                result.changed = true;
            }
        }
        result
    }

    fn flush_history(&self, finish: bool) {
        let mut logger = self.logger.lock().unwrap();
        if let Some(logger) = logger.as_mut() {
            let term = self.term.lock();
            let limit = self.scrollback.load(Ordering::Relaxed);
            if finish {
                logger.finish(&term, limit);
            } else {
                logger.flush(&term, limit);
            }
        }
    }
}

pub struct TerminalSession {
    pub shared: Arc<PaneShared>,
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    sender: EventLoopSender,
    events: Receiver<Event>,
    pub title: String,
    pub exited: bool,
    size: GridSize,
}

impl TerminalSession {
    pub fn spawn(
        ctx: &egui::Context,
        size: GridSize,
        scrollback: usize,
        settings: &Settings,
        id: u64,
    ) -> anyhow::Result<TerminalSession> {
        let (tx, events) = mpsc::channel();
        let signals = Arc::new(Signals::new());
        let proxy = EventProxy { tx, ctx: ctx.clone(), signals: signals.clone() };

        let config = TermConfig { scrolling_history: scrollback, ..TermConfig::default() };
        let term = Term::new(config, &TermSize::new(size.cols as usize, size.rows as usize), proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        // 状态文件按 App 进程 + 窗格编号区分;先清掉可能的残留
        let state_file = paths::run_dir().join(format!("{}-{id}.state", std::process::id()));
        let _ = std::fs::create_dir_all(paths::run_dir());
        let _ = std::fs::remove_file(&state_file);

        let program = shell::default_shell();
        let options = tty::Options {
            shell: Some(tty::Shell::new(program.clone(), login_args(&program))),
            working_directory: Some(paths::home_dir()),
            env: shell::shell_environment(&program, settings, &state_file).into_iter().collect(),
            ..tty::Options::default()
        };
        let pty = tty::new(&options, size.window_size(), id)?;
        #[cfg(unix)]
        let (shell_pid, probe) = (Some(pty.child().id()), PtyProbe::new(pty.file()));
        #[cfg(not(unix))]
        let (shell_pid, probe) = (None, None);

        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let sender = event_loop.channel();
        event_loop.spawn();

        let shared = Arc::new(PaneShared {
            term: term.clone(),
            signals,
            status: Mutex::new(StatusState::default()),
            usage_watcher: Mutex::new(UsageWatcher::default()),
            usage: Mutex::new(None),
            logger: Mutex::new(settings.save_history.then(|| SessionLogger::new(paths::history_dir()))),
            cwd: Mutex::new(Some(paths::home_dir())),
            probe,
            shell_pid,
            state_file,
            scrollback: AtomicUsize::new(scrollback),
            neutral: AtomicBool::new(false),
        });

        Ok(TerminalSession {
            shared,
            term,
            sender,
            events,
            title: short_name(&program),
            exited: false,
            size,
        })
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    /// 只把文本打到光标处,不回车(语音上屏的唯一入口,执行永远由用户按回车)
    pub fn type_text(&self, text: &str) {
        self.term.lock().scroll_display(Scroll::Bottom);
        self.write(text.as_bytes().to_vec());
    }

    /// 改这个窗格保留的历史行数,立即生效(缩小会马上丢掉最老的行)
    pub fn set_scrollback(&self, lines: usize) {
        self.shared.scrollback.store(lines, Ordering::Relaxed);
        self.term.lock().set_options(TermConfig { scrolling_history: lines, ..TermConfig::default() });
    }

    /// ⌘K:发 Ctrl-L,shell 重绘并清屏
    pub fn clear_screen(&self) {
        self.write(&b"\x0c"[..]);
    }

    pub fn resize(&mut self, size: GridSize) {
        if size == self.size || size.cols == 0 || size.rows == 0 {
            return;
        }
        self.size = size;
        self.term.lock().resize(TermSize::new(size.cols as usize, size.rows as usize));
        let _ = self.sender.send(Msg::Resize(size.window_size()));
    }

    /// 处理终端发回来的事件(标题、需要回写 PTY 的查询应答…)
    pub fn pump_events(&mut self, theme: &Theme, clipboard_out: &mut Option<String>) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Title(t) => self.title = t,
                Event::ResetTitle => self.title = short_name(&shell::default_shell()),
                Event::PtyWrite(s) => self.write(s.into_bytes()),
                Event::ClipboardStore(_, s) => *clipboard_out = Some(s),
                Event::ColorRequest(index, reply) => {
                    let rgb = self.term.lock().colors()[index].unwrap_or_else(|| {
                        let c = super::view::resolve_index(theme, index);
                        AnsiRgb { r: c.0, g: c.1, b: c.2 }
                    });
                    self.write(reply(rgb).into_bytes());
                }
                Event::TextAreaSizeRequest(reply) => {
                    self.write(reply(self.size.window_size()).into_bytes())
                }
                Event::ChildExit(_) | Event::Exit => self.exited = true,
                _ => {}
            }
        }
    }

    /// shell 当前所在目录(喂给命令生成)。不依赖 shell 集成,直接问操作系统。
    pub fn current_directory(&self) -> Option<PathBuf> {
        self.shared.shell_pid.and_then(proc::process_cwd)
    }

    /// 标签页显示名:当前目录的文件夹名(家目录显示 ~),拿不到时用终端标题
    pub fn display_name(&self) -> String {
        match self.shared.cwd() {
            Some(dir) if dir == paths::home_dir() => "~".into(),
            Some(dir) => dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into()),
            None => self.title.clone(),
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // 窗格关闭前把屏幕上剩下的行也写进记录
        self.shared.flush_history(true);
        let _ = std::fs::remove_file(&self.shared.state_file);
        let _ = self.sender.send(Msg::Shutdown);
    }
}

fn short_name(program: &str) -> String {
    std::path::Path::new(program)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string())
}

/// 类 Unix 下以登录 shell 启动(读 .zprofile / .bash_profile,拿到完整 PATH)
fn login_args(program: &str) -> Vec<String> {
    if cfg!(windows) {
        return vec!["-NoLogo".into()];
    }
    match short_name(program).as_str() {
        "zsh" | "bash" | "fish" | "sh" | "ksh" => vec!["-l".into()],
        _ => vec![],
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 测试别往真实的 history 目录里写
    fn test_settings() -> Settings {
        Settings { save_history: false, ..Settings::default() }
    }

    fn screen_text(session: &TerminalSession) -> String {
        let term = session.term.lock();
        let mut out = String::new();
        let mut last_line = None;
        for cell in term.grid().display_iter() {
            if last_line.is_some_and(|l| l != cell.point.line) {
                out.push('\n');
            }
            last_line = Some(cell.point.line);
            out.push(cell.c);
        }
        out
    }

    fn wait_for(session: &mut TerminalSession, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut clip = None;
        while Instant::now() < deadline {
            session.pump_events(&Theme::DARK, &mut clip);
            if screen_text(session).contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// 端到端:真 shell + PTY + 模拟器。typed 文本不带回车,不会执行;回车后才执行
    #[test]
    fn runs_a_real_shell() {
        let ctx = egui::Context::default();
        let size = GridSize { cols: 100, rows: 30, cell_w: 8, cell_h: 16 };
        let mut s = TerminalSession::spawn(&ctx, size, 1000, &test_settings(), 99).unwrap();

        s.type_text("echo vg-$((40+2))");
        assert!(wait_for(&mut s, "echo vg-$((40+2))"), "typed text never echoed:\n{}", screen_text(&s));
        assert!(!screen_text(&s).contains("vg-42"), "typed text must not execute without Enter");

        s.write(&b"\r"[..]);
        assert!(wait_for(&mut s, "vg-42"), "command output missing:\n{}", screen_text(&s));

        s.write(&b"cd /tmp\r"[..]);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut cwd = None;
        while Instant::now() < deadline {
            cwd = s.current_directory();
            if cwd.as_deref().is_some_and(|p| p.ends_with("tmp")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(cwd.is_some_and(|p| p.ends_with("tmp")), "shell cwd not tracked");

        s.write(&b"exit\r"[..]);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut clip = None;
        while !s.exited && Instant::now() < deadline {
            s.pump_events(&Theme::DARK, &mut clip);
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(s.exited, "exit not detected");
    }

    /// 一帧 headless egui 渲染:终端控件能布局、resize、出图元且不 panic
    #[test]
    fn view_renders_headless() {
        use crate::terminal::view::{self, InputState, ViewOptions};
        let ctx = egui::Context::default();
        let size = GridSize { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
        let mut s = TerminalSession::spawn(&ctx, size, 1000, &test_settings(), 98).unwrap();
        s.type_text("中文 and ascii");
        assert!(wait_for(&mut s, "ascii"));

        let mut state = InputState::default();
        let theme = Theme::DARK;
        let opts = ViewOptions { theme: &theme, font_size: 13.0, hold: Duration::from_millis(250), ptt_enabled: true, grab_focus: true };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let out = view::show(ui, &mut s, &mut state, &opts);
            assert!(out.actions.is_empty());
        });
        assert!(!out.shapes.is_empty());
        out.textures_delta.clear(); // headless:没有渲染器消费纹理更新
        use alacritty_terminal::grid::Dimensions;
        let term = s.term.lock();
        assert!(term.grid().columns() > 80, "view should resize the grid to the available width");
    }

    /// 状态灯端到端:真 zsh 的钩子写状态文件 → 监视 tick → 黄 → 绿 + 该响提示音
    #[test]
    fn status_follows_shell_integration() {
        if !shell::default_shell().ends_with("zsh") {
            return; // 命令边界集成目前只装在 zsh 上
        }
        let ctx = egui::Context::default();
        let size = GridSize { cols: 100, rows: 30, cell_w: 8, cell_h: 16 };
        let mut s = TerminalSession::spawn(&ctx, size, 1000, &test_settings(), 97).unwrap();
        let state_file = s.shared.state_file.clone();

        let deadline = Instant::now() + Duration::from_secs(15);
        while !std::fs::read_to_string(&state_file).is_ok_and(|t| t.starts_with("D 0")) {
            assert!(Instant::now() < deadline, "shell integration never wrote the state file");
            std::thread::sleep(Duration::from_millis(100));
        }

        s.write(&b"sleep 1.5\r"[..]);
        let mut n = 1;
        let (mut saw_busy, mut chimed) = (false, false);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let r = s.shared.tick(n);
            n += 1;
            chimed |= r.chime;
            match s.shared.status() {
                SessionStatus::Busy => saw_busy = true,
                SessionStatus::Done if saw_busy => break,
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        assert!(saw_busy, "never went busy");
        assert_eq!(s.shared.status(), SessionStatus::Done);
        assert!(chimed, "command end should earn the chime");

        // 你盯着的窗格:归灰
        s.shared.neutral.store(true, Ordering::Relaxed);
        s.shared.tick(n);
        assert_eq!(s.shared.status(), SessionStatus::Normal);
        let mut clip = None;
        s.pump_events(&Theme::DARK, &mut clip);
    }

    /// 选择 + 复制端到端:真实 egui 指针事件双击一个词,再发复制事件,检查剪贴板输出
    #[test]
    fn double_click_selects_word_and_copies() {
        use crate::terminal::view::{self, InputState, ViewOptions};
        let ctx = egui::Context::default();
        let size = GridSize { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
        let mut s = TerminalSession::spawn(&ctx, size, 1000, &test_settings(), 96).unwrap();
        s.type_text("vgword");
        assert!(wait_for(&mut s, "vgword"));

        let mut state = InputState::default();
        let theme = Theme::DARK;
        let opts = ViewOptions { theme: &theme, font_size: 13.0, hold: Duration::from_millis(250), ptt_enabled: true, grab_focus: true };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let frame = |events: Vec<egui::Event>, s: &mut TerminalSession, state: &mut InputState| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                view::show(ui, s, state, &opts);
            });
            out.textures_delta.clear();
            out.platform_output.commands
        };
        frame(vec![], &mut s, &mut state); // 首帧:布局、获得焦点

        // 找到词的位置,算出屏幕坐标
        let font = egui::FontId::monospace(13.0);
        let (cw, ch) = ctx.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let text = screen_text(&s);
        let (row, line) = text.lines().enumerate().find(|(_, l)| l.contains("vgword")).unwrap();
        let byte = line.find("vgword").unwrap();
        let col = line[..byte].chars().count() + 2; // 点在词中间
        let pos = egui::pos2((col as f32 + 0.5) * cw, (row as f32 + 0.5) * ch);

        let click = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for _ in 0..2 {
            frame(vec![egui::Event::PointerMoved(pos), click(true)], &mut s, &mut state);
            frame(vec![click(false)], &mut s, &mut state);
        }
        assert_eq!(s.term.lock().selection_to_string().as_deref(), Some("vgword"));

        let copy_modifiers = if cfg!(target_os = "macos") { egui::Modifiers::MAC_CMD } else { egui::Modifiers::CTRL | egui::Modifiers::SHIFT };
        let commands = frame(
            vec![egui::Event::Key { key: egui::Key::C, physical_key: None, pressed: true, repeat: false, modifiers: copy_modifiers }, egui::Event::Copy],
            &mut s,
            &mut state,
        );
        assert!(
            commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "vgword")),
            "{commands:?}"
        );
    }

    /// 搜索端到端:输出两处标记,⌘F 先命中最新的,再往上是更早的,再往下回到最新的;找不到的报 NotFound
    #[test]
    fn search_walks_matches() {
        use crate::terminal::search::{SearchResult, SearchState};
        let ctx = egui::Context::default();
        let size = GridSize { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
        let s = TerminalSession::spawn(&ctx, size, 1000, &test_settings(), 95).unwrap();
        s.write(&b"printf 'vgfind-one\\nfiller\\nvgfind-two\\n'\r"[..]);
        // 标记出现两次:一次在敲进去的命令里,一次在输出里
        let deadline = Instant::now() + Duration::from_secs(15);
        while screen_text(&s).matches("vgfind-two").count() < 2 {
            assert!(Instant::now() < deadline, "{}", screen_text(&s));
            std::thread::sleep(Duration::from_millis(100));
        }

        let mut search = SearchState::default();
        search.query = "vgfind-".into();
        let selected = |s: &TerminalSession| s.term.lock().selection_to_string().unwrap_or_default();
        let row_of_selection = |s: &TerminalSession| {
            let term = s.term.lock();
            term.selection.as_ref().and_then(|sel| sel.to_range(&term)).map(|r| r.start.line.0)
        };

        assert_eq!(search.step(&s, false), SearchResult::Found);
        assert_eq!(selected(&s), "vgfind-");
        let newest = row_of_selection(&s).unwrap();
        assert_eq!(search.step(&s, false), SearchResult::Found);
        let older = row_of_selection(&s).unwrap();
        assert!(older < newest, "second hit should be above the first ({older} vs {newest})");
        assert_eq!(search.step(&s, true), SearchResult::Found);
        assert_eq!(row_of_selection(&s).unwrap(), newest);

        let mut missing = SearchState::default();
        missing.query = "zz-not-there-zz".into();
        assert_eq!(missing.step(&s, false), SearchResult::NotFound);
    }

    /// 分屏:两个窗格并排渲染,只有当前窗格拿键盘焦点;点另一个窗格会报告 clicked
    #[test]
    fn split_panes_focus_and_click() {
        use crate::split::{Axis, Node};
        use crate::terminal::view::{self, InputState, ViewOptions};
        let ctx = egui::Context::default();
        let size = GridSize { cols: 40, rows: 20, cell_w: 8, cell_h: 16 };
        let mut left = TerminalSession::spawn(&ctx, size, 100, &test_settings(), 93).unwrap();
        let mut right = TerminalSession::spawn(&ctx, size, 100, &test_settings(), 94).unwrap();
        let (mut in_l, mut in_r) = (InputState::default(), InputState::default());
        let tree = Node::Leaf(1u64).inserting(2, 1, Axis::Horizontal);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let theme = Theme::DARK;

        let mut frame = |events: Vec<egui::Event>| {
            let mut clicked = vec![];
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                for (id, rect) in tree.frames(ui.available_rect_before_wrap(), 2.0) {
                    let opts = ViewOptions {
                        theme: &theme,
                        font_size: 13.0,
                        hold: Duration::from_millis(250),
                        ptt_enabled: true,
                        grab_focus: id == 1,
                    };
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("pane", id)));
                    let (s, st) = if id == 1 { (&mut left, &mut in_l) } else { (&mut right, &mut in_r) };
                    if view::show(&mut child, s, st, &opts).clicked {
                        clicked.push(id);
                    }
                }
            });
            out.textures_delta.clear();
            clicked
        };
        frame(vec![]);
        frame(vec![]);
        let focused = ctx.memory(|m| m.focused());
        assert!(focused.is_some(), "active pane should own keyboard focus");

        // 点右半边
        let pos = egui::pos2(600.0, 200.0);
        let press = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let clicked = frame(vec![egui::Event::PointerMoved(pos), press(true)]);
        assert_eq!(clicked, vec![2]);
    }
}
