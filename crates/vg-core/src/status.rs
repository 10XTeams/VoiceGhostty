//! 窗格状态灯(四态):蓝 = 平常,黄 = 忙,绿 = 做完了、轮到你,橙 = **卡住等你输入**(权限确认、y/n、密码)。
//! 与 Swift 版 `TerminalSession.refreshStatus` 同一套规则,橙色是 Rust 版新增的。
//!
//! - **精确模式**(见过 shell 集成的命令边界):命令开始 → 黄,命令结束 → 绿,绿灯保持到你来看或下一条命令。
//!   例外:交互式全屏程序(claude、vim、less)对 shell 来说是一条很长的"命令",它的灯改由输出启发式决定,
//!   否则整个会话都是黄的。
//! - **兜底启发式**(没有 shell 集成):响铃,或"有过输出后安静 ≥ 阈值" → 绿;最近有输出 → 黄。
//!
//! 提示音比灯更严格:响铃和命令结束是程序**声明**自己做完了;"输出安静了"只是推断,在 TUI 里多半是错的——
//! claude 在两次工具调用之间停顿,看起来和做完一模一样。所以 TUI 安静下来只改灯、不响;
//! 兜底分支分不清这两种情况,也不去分,安静就响。

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionStatus {
    #[default]
    Normal,
    Busy,
    Done,
    /// 命令还在跑,但安静下来了,而且屏幕末尾像是在问你(「Do you want to…」「[y/N]」「Password:」)。
    /// 对 claude 来说就是等你批准工具调用——比「做完了」更需要你过去
    Waiting,
}

impl SessionStatus {
    /// 多个窗格汇总时的优先级:waiting > done > busy > normal
    pub fn priority(self) -> u8 {
        match self {
            SessionStatus::Normal => 0,
            SessionStatus::Busy => 1,
            SessionStatus::Done => 2,
            SessionStatus::Waiting => 3,
        }
    }

    /// 灯亮着且值得你过去看(绿或橙)
    pub fn needs_attention(self) -> bool {
        matches!(self, SessionStatus::Done | SessionStatus::Waiting)
    }

    pub fn label(self) -> &'static str {
        match self {
            SessionStatus::Normal => "空闲",
            SessionStatus::Busy => "执行中",
            SessionStatus::Done => "已完成",
            SessionStatus::Waiting => "等你输入",
        }
    }
}

/// 一次推进所需的外部观测
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    pub now: Instant,
    /// 前台是交互式全屏程序(tty 处于 raw 模式)。未知按"普通命令"处理
    pub interactive_app: bool,
    /// 屏幕末尾看起来像一个等待回答的提示(见 [`looks_like_prompt`])
    pub prompt_visible: bool,
}

impl Observation {
    pub fn new(now: Instant, interactive_app: bool) -> Observation {
        Observation { now, interactive_app, prompt_visible: false }
    }
}

/// 屏幕最后几行是不是一个在等你回答的提示。只看最后几行非空文本(小写、去首尾空白),
/// 命中其中一条即可。词表偏保守:宁可少亮橙灯,也别把普通输出当成提问
pub fn looks_like_prompt(last_lines: &[String]) -> bool {
    const NEEDLES: &[&str] = &[
        // claude code / 各类 agent 的权限确认
        "do you want to",
        "yes, and don't ask again",
        "allow once",
        "allow always",
        "esc to cancel",
        "enter to confirm",
        "press enter to",
        // 通用 y/n
        "(y/n)",
        "[y/n]",
        "[y/n]",
        "(yes/no)",
        "[yes/no]",
        "y/n?",
        "continue?",
        "proceed?",
        "overwrite?",
        "are you sure",
        // 密码
        "password:",
        "passphrase",
        // 中文
        "是否继续",
        "是否覆盖",
        "(是/否)",
        "确认吗",
        "继续吗",
        "请输入",
    ];
    last_lines
        .iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(8)
        .map(|l| l.trim().to_lowercase())
        .any(|l| NEEDLES.iter().any(|n| l.contains(n)))
}

#[derive(Debug, Default)]
pub struct StatusMachine {
    pub status: SessionStatus,
    last_output: Option<Instant>,
    has_activity: bool,
    bell_pending: bool,
    cmd_running: bool,
    cmd_ended_pending: bool,
    /// 一旦见过命令边界就切到精确模式
    integration_seen: bool,
}

impl StatusMachine {
    // MARK: 信号

    pub fn note_output(&mut self, at: Instant) {
        self.last_output = Some(at);
        self.has_activity = true;
    }

    pub fn note_bell(&mut self) {
        self.bell_pending = true;
    }

    /// 新命令从干净的活动状态开始,TUI 的灯只按它自己的输出判断
    pub fn note_command_start(&mut self) {
        self.cmd_running = true;
        self.integration_seen = true;
        self.has_activity = false;
    }

    pub fn note_command_end(&mut self) {
        self.integration_seen = true;
        if self.cmd_running {
            self.cmd_running = false;
            self.cmd_ended_pending = true;
        }
    }

    /// shell 正停在提示符上等你敲命令(只有见过 shell 集成才敢说是)
    pub fn at_prompt(&self) -> bool {
        self.integration_seen && !self.cmd_running
    }

    /// shell 集成报告有命令在跑(没有集成时按输出活动推断:最近有过输出就算)
    pub fn is_command_running(&self) -> bool {
        if self.integration_seen { self.cmd_running } else { self.has_activity }
    }

    // MARK: 推进

    /// 你正在用的那个终端保持中性(不拿自己的灯烦自己)。`cmd_running` 反映真实状态,不清。
    /// 返回状态是否变化。
    pub fn reset(&mut self) -> bool {
        let changed = self.status != SessionStatus::Normal
            || self.has_activity
            || self.bell_pending
            || self.cmd_ended_pending;
        self.status = SessionStatus::Normal;
        self.has_activity = false;
        self.bell_pending = false;
        self.cmd_ended_pending = false;
        changed
    }

    /// 你正盯着的那个窗格:忙照常亮黄(敲下回车就该看到它在干活),但绿 / 橙不亮、也不响——
    /// 你就在这儿,不需要提醒。返回状态是否变化。
    pub fn refresh_watched(&mut self, obs: Observation, silence: Duration) -> bool {
        let old = self.status;
        self.refresh(obs, silence);
        if self.status.needs_attention() {
            self.reset();
        }
        self.status != old
    }

    /// 返回 `(状态是否变化, 这次变绿是否值得响提示音)`
    pub fn refresh(&mut self, obs: Observation, silence: Duration) -> (bool, bool) {
        let old = self.status;
        let mut chimeworthy = false;
        if self.integration_seen {
            if self.bell_pending {
                self.bell_pending = false;
                self.cmd_ended_pending = false;
                self.status = SessionStatus::Done;
                chimeworthy = true;
            } else if self.cmd_running {
                if obs.interactive_app {
                    self.advance_by_output(obs.now, silence);
                } else {
                    self.status = SessionStatus::Busy;
                }
                // 命令还在跑却安静了,屏幕末尾又像在提问:它在等你。TUI 安静本来不响,
                // 但「看见提问」是明确的证据,值得响一声
                if self.is_quiet(obs.now, silence) && obs.prompt_visible {
                    if self.status != SessionStatus::Waiting {
                        chimeworthy = true;
                    }
                    self.status = SessionStatus::Waiting;
                } else if self.status == SessionStatus::Waiting {
                    // 提示消失(你答了)或又有输出:回到忙
                    self.status = SessionStatus::Busy;
                }
            } else if self.cmd_ended_pending {
                self.cmd_ended_pending = false;
                self.status = SessionStatus::Done;
                chimeworthy = true;
            }
            // 否则保持:做完的命令一直绿,直到你来看或开始下一条
        } else if self.bell_pending {
            self.bell_pending = false;
            self.has_activity = false;
            self.status = SessionStatus::Done;
            chimeworthy = true;
        } else {
            self.advance_by_output(obs.now, silence);
            chimeworthy = self.status == SessionStatus::Done;
        }
        let turned_on = !old.needs_attention() && self.status.needs_attention();
        let turned_waiting = old != SessionStatus::Waiting && self.status == SessionStatus::Waiting;
        (self.status != old, (turned_on || turned_waiting) && chimeworthy)
    }

    /// 自上次输出已经安静了 `silence`(从没输出过也算安静)
    fn is_quiet(&self, now: Instant, silence: Duration) -> bool {
        self.last_output.is_none_or(|t| now.duration_since(t) >= silence)
    }

    /// 最近有输出 → 忙;有过输出然后安静了 `silence` → 做完。完全没输出过就不动
    fn advance_by_output(&mut self, now: Instant, silence: Duration) {
        if !self.has_activity {
            return;
        }
        let quiet = self.last_output.is_none_or(|t| now.duration_since(t) >= silence);
        if quiet {
            self.status = SessionStatus::Done;
            self.has_activity = false; // 做完后,直到下一次输出前不再判断
        } else {
            self.status = SessionStatus::Busy;
        }
    }
}

/// shell 集成写的状态文件内容:`C <n>`(第 n 条命令开始)或 `D <n>`(第 n 条命令结束后回到提示符)。
/// 用序号而不是只看 C/D:两次轮询之间跑完的快命令,只会留下 `D n`,序号变了就知道中间发生过一条命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMark {
    Started(u64),
    Prompt(u64),
}

impl ShellMark {
    pub fn parse(text: &str) -> Option<ShellMark> {
        StateFile::parse(text).map(|f| f.mark)
    }
}

/// 状态文件的完整内容。首行 `C <n>` 后面跟着命令原文(可能多行);`D <n> <exit>` 带上一条命令的退出码
/// (老版本的钩子没有退出码,也照样解析)。命令原文与退出码喂给自然语言模式的 LLM,让「刚才为什么报错」
/// 这类话有所指。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFile {
    pub mark: ShellMark,
    pub exit_code: Option<i32>,
    pub command: Option<String>,
}

impl StateFile {
    pub fn parse(text: &str) -> Option<StateFile> {
        let (head, rest) = match text.split_once('\n') {
            Some((h, r)) => (h, Some(r)),
            None => (text, None),
        };
        let mut parts = head.split_whitespace();
        let kind = parts.next()?;
        let n = parts.next()?.parse().ok()?;
        let third = parts.next().and_then(|s| s.parse::<i32>().ok());
        match kind {
            "C" => Some(StateFile {
                mark: ShellMark::Started(n),
                exit_code: None,
                command: rest.map(str::trim).filter(|c| !c.is_empty()).map(str::to_string),
            }),
            "D" => Some(StateFile { mark: ShellMark::Prompt(n), exit_code: third, command: None }),
            _ => None,
        }
    }
}

/// 最近一条经 shell 集成看到的命令:原文与(回到提示符后)它的退出码
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCommand {
    pub seq: u64,
    pub text: String,
    pub exit_code: Option<i32>,
}

impl LastCommand {
    /// 把状态文件的一次读取并进来:新命令开始 → 换成它;回到提示符 → 补上退出码
    pub fn absorb(slot: &mut Option<LastCommand>, file: &StateFile) {
        match file.mark {
            ShellMark::Started(n) => {
                if let Some(text) = &file.command {
                    if slot.as_ref().is_none_or(|c| c.seq != n) {
                        *slot = Some(LastCommand { seq: n, text: text.clone(), exit_code: None });
                    }
                }
            }
            ShellMark::Prompt(n) => {
                if let Some(c) = slot.as_mut() {
                    if c.seq == n && c.exit_code.is_none() {
                        c.exit_code = file.exit_code;
                    }
                }
            }
        }
    }
}

/// 把状态文件的变化翻译成命令开始/结束信号
#[derive(Debug, Default)]
pub struct ShellMarkTracker {
    last: Option<ShellMark>,
}

impl ShellMarkTracker {
    pub fn apply(&mut self, mark: ShellMark, machine: &mut StatusMachine) {
        if self.last == Some(mark) {
            return;
        }
        let prev_seq = match self.last {
            Some(ShellMark::Started(n) | ShellMark::Prompt(n)) => n,
            None => 0,
        };
        match mark {
            ShellMark::Started(n) => {
                if n > prev_seq && matches!(self.last, Some(ShellMark::Started(_))) {
                    // 上一条在两次轮询之间结束了,又开始了新的一条
                    machine.note_command_start();
                    machine.note_command_end();
                }
                machine.note_command_start();
            }
            ShellMark::Prompt(n) => {
                if n > 0 {
                    if n > prev_seq && !matches!(self.last, Some(ShellMark::Started(m)) if m == n) {
                        // 整条命令都落在两次轮询之间:补一个开始
                        machine.note_command_start();
                    }
                    machine.note_command_end();
                } else {
                    // `D 0`:shell 刚启动到第一个提示符,只说明集成在工作
                    machine.integration_seen = true;
                }
            }
        }
        self.last = Some(mark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SILENCE: Duration = Duration::from_secs(2);

    fn obs(now: Instant) -> Observation {
        Observation::new(now, false)
    }

    #[test]
    fn heuristic_busy_then_done_with_chime() {
        let mut m = StatusMachine::default();
        let t0 = Instant::now();
        m.note_output(t0);
        assert_eq!(m.refresh(obs(t0 + Duration::from_millis(500)), SILENCE), (true, false));
        assert_eq!(m.status, SessionStatus::Busy);
        assert_eq!(m.refresh(obs(t0 + Duration::from_secs(3)), SILENCE), (true, true));
        assert_eq!(m.status, SessionStatus::Done);
        // 保持绿,不重复响
        assert_eq!(m.refresh(obs(t0 + Duration::from_secs(4)), SILENCE), (false, false));
    }

    #[test]
    fn precise_mode_command_boundaries() {
        let mut m = StatusMachine::default();
        let t0 = Instant::now();
        m.note_command_start();
        m.refresh(obs(t0), SILENCE);
        assert_eq!(m.status, SessionStatus::Busy);
        m.note_command_end();
        assert_eq!(m.refresh(obs(t0), SILENCE), (true, true));
        assert_eq!(m.status, SessionStatus::Done);
    }

    #[test]
    fn tui_going_quiet_lights_green_without_chime() {
        let mut m = StatusMachine::default();
        let t0 = Instant::now();
        m.note_command_start();
        m.note_output(t0);
        let tui = |now| Observation::new(now, true);
        m.refresh(tui(t0), SILENCE);
        assert_eq!(m.status, SessionStatus::Busy);
        assert_eq!(m.refresh(tui(t0 + Duration::from_secs(3)), SILENCE), (true, false));
        assert_eq!(m.status, SessionStatus::Done);
        // TUI 响铃(claude 做完会响)才值得提示音
        m.note_output(t0 + Duration::from_secs(4));
        m.refresh(tui(t0 + Duration::from_secs(4)), SILENCE);
        m.note_bell();
        assert_eq!(m.refresh(tui(t0 + Duration::from_secs(5)), SILENCE), (true, true));
    }

    #[test]
    fn quiet_prompt_turns_waiting_with_chime_then_back_to_busy() {
        let mut m = StatusMachine::default();
        let t0 = Instant::now();
        m.note_command_start();
        m.note_output(t0);
        let tui = |now, prompt| Observation { now, interactive_app: true, prompt_visible: prompt };
        assert_eq!(m.refresh(tui(t0, true), SILENCE), (true, false)); // 刚有输出:忙,提示还不算
        assert_eq!(m.status, SessionStatus::Busy);
        assert_eq!(m.refresh(tui(t0 + Duration::from_secs(3), true), SILENCE), (true, true));
        assert_eq!(m.status, SessionStatus::Waiting);
        // 保持等待,不重复响
        assert_eq!(m.refresh(tui(t0 + Duration::from_secs(4), true), SILENCE), (false, false));
        // 你回答了,程序又开始输出
        m.note_output(t0 + Duration::from_secs(5));
        assert_eq!(m.refresh(tui(t0 + Duration::from_secs(5), false), SILENCE), (true, false));
        assert_eq!(m.status, SessionStatus::Busy);
        // 普通命令(非 raw)安静 + 提示 → 同样等待
        let mut m2 = StatusMachine::default();
        m2.note_command_start();
        m2.note_output(t0);
        m2.refresh(Observation { now: t0 + Duration::from_secs(3), interactive_app: false, prompt_visible: true }, SILENCE);
        assert_eq!(m2.status, SessionStatus::Waiting);
        assert!(SessionStatus::Waiting.priority() > SessionStatus::Done.priority());
    }

    #[test]
    fn prompt_detection() {
        let lines = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(looks_like_prompt(&lines(&["Edit file src/main.rs", "", "Do you want to make this edit?", " ❯ 1. Yes", "   2. Yes, and don't ask again", "   3. No"])));
        assert!(looks_like_prompt(&lines(&["rm: remove regular file 'a.txt'? [y/N]"])));
        assert!(looks_like_prompt(&lines(&["Password:"])));
        assert!(looks_like_prompt(&lines(&["是否继续?(是/否)"])));
        assert!(!looks_like_prompt(&lines(&["Compiling vg-core v0.1.0", "Finished dev profile"])));
        assert!(!looks_like_prompt(&lines(&["$ "])));
        // 只看最后 8 行:老早的提问不算
        let mut old: Vec<String> = lines(&["Do you want to continue?"]);
        old.extend((0..9).map(|i| format!("line {i}")));
        assert!(!looks_like_prompt(&old));
    }

    #[test]
    fn reset_neutralizes() {
        let mut m = StatusMachine::default();
        m.note_bell();
        m.refresh(obs(Instant::now()), SILENCE);
        assert!(m.reset());
        assert_eq!(m.status, SessionStatus::Normal);
        assert!(!m.reset());
    }

    #[test]
    fn watched_pane_shows_busy_but_never_attention() {
        let mut m = StatusMachine::default();
        let t0 = Instant::now();
        m.note_command_start();
        assert!(m.refresh_watched(obs(t0), SILENCE));
        assert_eq!(m.status, SessionStatus::Busy);
        m.note_command_end();
        assert!(m.refresh_watched(obs(t0), SILENCE));
        assert_eq!(m.status, SessionStatus::Normal);
        // 做完的那一下已经被你看到了:之后切走也不会补亮绿灯
        assert_eq!(m.refresh(obs(t0), SILENCE), (false, false));
        // TUI:有输出 → 黄,安静 → 回到平常;在提问也不亮橙
        m.note_command_start();
        m.note_output(t0);
        let tui = |now| Observation { now, interactive_app: true, prompt_visible: true };
        m.refresh_watched(tui(t0), SILENCE);
        assert_eq!(m.status, SessionStatus::Busy);
        m.refresh_watched(tui(t0 + Duration::from_secs(3)), SILENCE);
        assert_eq!(m.status, SessionStatus::Normal);
    }

    #[test]
    fn parses_marks() {
        assert_eq!(ShellMark::parse("C 3\n"), Some(ShellMark::Started(3)));
        assert_eq!(ShellMark::parse("D 0"), Some(ShellMark::Prompt(0)));
        assert_eq!(ShellMark::parse("D 2 127"), Some(ShellMark::Prompt(2)));
        assert_eq!(ShellMark::parse("X 1"), None);
        assert_eq!(ShellMark::parse(""), None);
    }

    #[test]
    fn parses_state_file_with_command_and_exit_code() {
        let c = StateFile::parse("C 3\ngit push -u origin main\n").unwrap();
        assert_eq!(c.mark, ShellMark::Started(3));
        assert_eq!(c.command.as_deref(), Some("git push -u origin main"));
        assert_eq!(c.exit_code, None);
        let d = StateFile::parse("D 3 1\n").unwrap();
        assert_eq!(d.mark, ShellMark::Prompt(3));
        assert_eq!(d.exit_code, Some(1));
        assert_eq!(StateFile::parse("D 3").unwrap().exit_code, None);

        let mut slot = None;
        LastCommand::absorb(&mut slot, &c);
        assert_eq!(slot.as_ref().unwrap().text, "git push -u origin main");
        LastCommand::absorb(&mut slot, &d);
        assert_eq!(slot.as_ref().unwrap().exit_code, Some(1));
        // 序号对不上的退出码不乱认
        LastCommand::absorb(&mut slot, &StateFile::parse("D 9 0").unwrap());
        assert_eq!(slot.as_ref().unwrap().exit_code, Some(1));
    }

    #[test]
    fn tracker_sees_fast_command_between_polls() {
        let mut m = StatusMachine::default();
        let mut t = ShellMarkTracker::default();
        t.apply(ShellMark::Prompt(0), &mut m);
        // 一条命令整个在两次轮询之间跑完:只看到 D 1
        t.apply(ShellMark::Prompt(1), &mut m);
        assert_eq!(m.refresh(obs(Instant::now()), SILENCE), (true, true));
        assert_eq!(m.status, SessionStatus::Done);
    }

    #[test]
    fn tracker_normal_sequence() {
        let mut m = StatusMachine::default();
        let mut t = ShellMarkTracker::default();
        t.apply(ShellMark::Prompt(0), &mut m);
        t.apply(ShellMark::Started(1), &mut m);
        m.refresh(obs(Instant::now()), SILENCE);
        assert_eq!(m.status, SessionStatus::Busy);
        t.apply(ShellMark::Started(1), &mut m); // 重复读到同一内容:无事发生
        t.apply(ShellMark::Prompt(1), &mut m);
        assert_eq!(m.refresh(obs(Instant::now()), SILENCE), (true, true));
    }
}
