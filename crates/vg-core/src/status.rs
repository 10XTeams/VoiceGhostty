//! 窗格状态灯(三态):灰 = 平常,黄 = 忙,绿 = 做完了、轮到你。与 Swift 版 `TerminalSession.refreshStatus` 同一套规则。
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
}

impl SessionStatus {
    /// 多个窗格汇总时的优先级:done > busy > normal
    pub fn priority(self) -> u8 {
        match self {
            SessionStatus::Normal => 0,
            SessionStatus::Busy => 1,
            SessionStatus::Done => 2,
        }
    }
}

/// 一次推进所需的外部观测
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    pub now: Instant,
    /// 前台是交互式全屏程序(tty 处于 raw 模式)。未知按"普通命令"处理
    pub interactive_app: bool,
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
        let turned_done = old != SessionStatus::Done && self.status == SessionStatus::Done;
        (self.status != old, turned_done && chimeworthy)
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
        let mut parts = text.split_whitespace();
        let kind = parts.next()?;
        let n = parts.next()?.parse().ok()?;
        match kind {
            "C" => Some(ShellMark::Started(n)),
            "D" => Some(ShellMark::Prompt(n)),
            _ => None,
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
        Observation { now, interactive_app: false }
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
        let tui = |now| Observation { now, interactive_app: true };
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
    fn reset_neutralizes() {
        let mut m = StatusMachine::default();
        m.note_bell();
        m.refresh(obs(Instant::now()), SILENCE);
        assert!(m.reset());
        assert_eq!(m.status, SessionStatus::Normal);
        assert!(!m.reset());
    }

    #[test]
    fn parses_marks() {
        assert_eq!(ShellMark::parse("C 3\n"), Some(ShellMark::Started(3)));
        assert_eq!(ShellMark::parse("D 0"), Some(ShellMark::Prompt(0)));
        assert_eq!(ShellMark::parse("X 1"), None);
        assert_eq!(ShellMark::parse(""), None);
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
