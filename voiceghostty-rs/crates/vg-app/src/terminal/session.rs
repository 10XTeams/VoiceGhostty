//! 一个终端会话 = 一个 PTY + 一个 shell + 一个 alacritty 终端状态机。
//! PTY 读写在 alacritty 自己的 IO 线程里跑;UI 线程只在绘制时短暂加锁读网格。

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::Rgb as AnsiRgb;

use vg_core::{shell, Settings, Theme};

/// 把终端事件转回 UI 线程,并唤醒 egui 重绘
#[derive(Clone)]
pub struct EventProxy {
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if !matches!(event, Event::Wakeup) {
            let _ = self.tx.send(event);
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

pub struct TerminalSession {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    sender: EventLoopSender,
    events: Receiver<Event>,
    pub title: String,
    pub exited: bool,
    /// 有新输出后置位,UI 用来做"忙/完成"提示;bell 另计
    pub bell: bool,
    size: GridSize,
    shell_pid: Option<u32>,
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
        let proxy = EventProxy { tx, ctx: ctx.clone() };

        let config = TermConfig { scrolling_history: scrollback, ..TermConfig::default() };
        let term = Term::new(config, &TermSize::new(size.cols as usize, size.rows as usize), proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let program = shell::default_shell();
        let options = tty::Options {
            shell: Some(tty::Shell::new(program.clone(), login_args(&program))),
            working_directory: Some(vg_core::paths::home_dir()),
            env: shell::shell_environment(&program, settings).into_iter().collect(),
            ..tty::Options::default()
        };
        let pty = tty::new(&options, size.window_size(), id)?;
        #[cfg(unix)]
        let shell_pid = Some(pty.child().id());
        #[cfg(not(unix))]
        let shell_pid = None;

        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let sender = event_loop.channel();
        event_loop.spawn();

        Ok(TerminalSession {
            term,
            sender,
            events,
            title: short_name(&program),
            exited: false,
            bell: false,
            size,
            shell_pid,
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

    pub fn resize(&mut self, size: GridSize) {
        if size == self.size || size.cols == 0 || size.rows == 0 {
            return;
        }
        self.size = size;
        self.term.lock().resize(TermSize::new(size.cols as usize, size.rows as usize));
        let _ = self.sender.send(Msg::Resize(size.window_size()));
    }

    /// 处理终端发回来的事件(标题、响铃、需要回写 PTY 的查询应答…)
    pub fn pump_events(&mut self, theme: &Theme, clipboard_out: &mut Option<String>) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Title(t) => self.title = t,
                Event::ResetTitle => self.title = short_name(&shell::default_shell()),
                Event::Bell => self.bell = true,
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
        process_cwd(self.shell_pid?)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
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

#[cfg(target_os = "linux")]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "macos")]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if n != size {
        return None;
    }
    let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr() as *const libc::c_char) };
    Some(PathBuf::from(path.to_string_lossy().into_owned()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

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
        let mut s = TerminalSession::spawn(&ctx, size, 1000, &Settings::default(), 99).unwrap();

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
        let mut s = TerminalSession::spawn(&ctx, size, 1000, &Settings::default(), 98).unwrap();
        s.type_text("中文 and ascii");
        assert!(wait_for(&mut s, "ascii"));

        let mut state = InputState::default();
        let theme = Theme::DARK;
        let opts = ViewOptions { theme: &theme, font_size: 13.0, hold: Duration::from_millis(250), ptt_enabled: true };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let actions = view::show(ui, &mut s, &mut state, &opts);
            assert!(actions.is_empty());
        });
        assert!(!out.shapes.is_empty());
        out.textures_delta.clear(); // headless:没有渲染器消费纹理更新
        use alacritty_terminal::grid::Dimensions;
        let term = s.term.lock();
        assert!(term.grid().columns() > 80, "view should resize the grid to the available width");
    }
}
