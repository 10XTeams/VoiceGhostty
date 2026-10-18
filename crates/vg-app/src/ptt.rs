//! 按住空格说话的状态机(与 GUI 无关,便于单测):
//! - 快速点按(< 阈值)→ 普通空格,松开时补发
//! - 按住(≥ 阈值)→ 开始录音,松开停止
//! - 按住期间系统的按键重复全部吞掉

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttAction {
    /// 什么都不做(事件已被吞掉)
    None,
    StartRecording,
    StopRecording,
    /// 是一次普通点按:把空格发给终端
    TypeSpace,
}

#[derive(Debug, Default)]
pub struct PushToTalk {
    pressed_at: Option<Instant>,
    holding: bool,
}

impl PushToTalk {
    #[cfg(test)]
    pub fn is_holding(&self) -> bool {
        self.holding
    }

    pub fn is_pending(&self) -> bool {
        self.pressed_at.is_some() && !self.holding
    }

    pub fn key_down(&mut self, now: Instant) {
        if self.pressed_at.is_none() && !self.holding {
            self.pressed_at = Some(now);
        }
    }

    /// 每帧调用:按住超过阈值就开始录音
    pub fn tick(&mut self, now: Instant, threshold: Duration) -> PttAction {
        match self.pressed_at {
            Some(t) if !self.holding && now.duration_since(t) >= threshold => {
                self.holding = true;
                PttAction::StartRecording
            }
            _ => PttAction::None,
        }
    }

    pub fn key_up(&mut self, now: Instant, threshold: Duration) -> PttAction {
        // 先给 tick 一次机会:两帧之间按过了阈值又松开,仍算按住说话
        let started = self.tick(now, threshold) == PttAction::StartRecording;
        let was_pressed = self.pressed_at.take().is_some();
        if self.holding {
            self.holding = false;
            // 刚好在这一刻才跨过阈值:录音区间为零,等同误触,直接当空格
            return if started { PttAction::TypeSpace } else { PttAction::StopRecording };
        }
        if was_pressed { PttAction::TypeSpace } else { PttAction::None }
    }

    /// 窗口失焦等情况:放弃当前按键
    pub fn cancel(&mut self) -> PttAction {
        self.pressed_at = None;
        if std::mem::take(&mut self.holding) {
            PttAction::StopRecording
        } else {
            PttAction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_millis(250);

    #[test]
    fn quick_tap_types_space() {
        let mut p = PushToTalk::default();
        let t0 = Instant::now();
        p.key_down(t0);
        assert_eq!(p.tick(t0 + Duration::from_millis(100), T), PttAction::None);
        assert_eq!(p.key_up(t0 + Duration::from_millis(120), T), PttAction::TypeSpace);
    }

    #[test]
    fn hold_records_and_release_stops() {
        let mut p = PushToTalk::default();
        let t0 = Instant::now();
        p.key_down(t0);
        assert_eq!(p.tick(t0 + Duration::from_millis(300), T), PttAction::StartRecording);
        // 按键重复不重置
        p.key_down(t0 + Duration::from_millis(350));
        assert_eq!(p.tick(t0 + Duration::from_millis(400), T), PttAction::None);
        assert_eq!(p.key_up(t0 + Duration::from_millis(2000), T), PttAction::StopRecording);
        assert!(!p.is_holding());
    }

    #[test]
    fn cancel_while_holding_stops() {
        let mut p = PushToTalk::default();
        let t0 = Instant::now();
        p.key_down(t0);
        p.tick(t0 + Duration::from_millis(300), T);
        assert_eq!(p.cancel(), PttAction::StopRecording);
        assert_eq!(p.key_up(t0 + Duration::from_millis(400), T), PttAction::None);
    }
}
