//! 用量洞察的后台线程:定时扫描 Claude Code 的会话记录、整理台账,面板从这里拿整理好的数据。
//!
//! 三个触发点:启动时距上次成功扫描超过一个周期就补扫;运行期间每个周期一次(墙上时钟判断,
//! 休眠期间线程不跑);打开面板时总是先扫一次。扫描本身在这个线程里做,不卡界面和监视线程。

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vg_core::insights::{claude, ledger, scan, TurnRecord};

/// 面板读的快照
#[derive(Default, Clone)]
pub struct Snapshot {
    pub records: Arc<Vec<TurnRecord>>,
    /// 正在扫描:(已处理文件, 总数)
    pub progress: Option<(usize, usize)>,
    pub last_scan_ms: Option<i64>,
    pub error: Option<String>,
    /// 记录里见到的 Claude Code 版本比验证过的新
    pub unverified_version: Option<String>,
    /// 每次数据变了加一,面板据此决定要不要重算汇总
    pub generation: u64,
}

enum Msg {
    ScanNow,
    Period(u32),
    Background(bool),
    Clear,
}

pub struct Insights {
    state: Arc<Mutex<Snapshot>>,
    tx: Sender<Msg>,
}

impl Insights {
    /// `background`:是否定时扫描(设置里关掉时只在打开面板时扫)
    pub fn start(ctx: egui::Context, period_minutes: u32, background: bool) -> Insights {
        Self::start_with(ctx, claude::projects_root(), ledger::usage_dir(), period_minutes, background)
    }

    pub fn start_with(ctx: egui::Context, source: PathBuf, dir: PathBuf, period_minutes: u32, background: bool) -> Insights {
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let (tx, rx) = mpsc::channel();
        let shared = state.clone();
        std::thread::Builder::new()
            .name("insights".into())
            .spawn(move || {
                let mut background = background;
                let mut period = period_ms(period_minutes);
                reload(&shared, &dir, &ctx);
                let mut want = background && scan::due(&dir, period);
                loop {
                    if want {
                        run_scan(&shared, &source, &dir, &ctx);
                        want = false;
                    }
                    match rx.recv_timeout(Duration::from_secs(60)) {
                        Ok(Msg::ScanNow) => want = true,
                        Ok(Msg::Period(m)) => period = period_ms(m),
                        Ok(Msg::Background(on)) => {
                            background = on;
                            want = on && scan::due(&dir, period);
                        }
                        Ok(Msg::Clear) => {
                            if let Err(e) = ledger::clear(&dir) {
                                shared.lock().unwrap().error = Some(format!("清除失败:{e}"));
                            }
                            reload(&shared, &dir, &ctx);
                        }
                        Err(RecvTimeoutError::Timeout) => want = background && scan::due(&dir, period),
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("spawn insights thread");
        Insights { state, tx }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }

    /// 截图 / 测试用:直接换成给定的数据
    #[cfg(test)]
    pub fn replace(&self, snap: Snapshot) {
        let mut s = self.state.lock().unwrap();
        let generation = s.generation + 1;
        *s = Snapshot { generation, ..snap };
    }

    pub fn scan_now(&self) {
        let _ = self.tx.send(Msg::ScanNow);
    }

    pub fn set_period(&self, minutes: u32) {
        let _ = self.tx.send(Msg::Period(minutes));
    }

    pub fn set_background(&self, on: bool) {
        let _ = self.tx.send(Msg::Background(on));
    }

    pub fn clear(&self) {
        let _ = self.tx.send(Msg::Clear);
    }
}

fn period_ms(minutes: u32) -> i64 {
    minutes.max(5) as i64 * 60_000
}

fn run_scan(shared: &Arc<Mutex<Snapshot>>, source: &std::path::Path, dir: &std::path::Path, ctx: &egui::Context) {
    shared.lock().unwrap().progress = Some((0, 0));
    ctx.request_repaint();
    let mut last_paint = std::time::Instant::now();
    let result = scan::scan(source, dir, |done, total| {
        shared.lock().unwrap().progress = Some((done, total));
        if last_paint.elapsed() > Duration::from_millis(200) {
            ctx.request_repaint();
            last_paint = std::time::Instant::now();
        }
    });
    {
        let mut s = shared.lock().unwrap();
        s.progress = None;
        match result {
            Ok(report) => {
                s.error = None;
                s.unverified_version = report
                    .newest_version
                    .filter(|v| scan::newer(v, claude::VERIFIED_VERSION));
            }
            Err(e) => s.error = Some(format!("整理失败:{e}")),
        }
    }
    reload(shared, dir, ctx);
}

fn reload(shared: &Arc<Mutex<Snapshot>>, dir: &std::path::Path, ctx: &egui::Context) {
    let records = ledger::load(dir, 0);
    let last = ledger::Cursors::load(dir).last_scan_ms;
    let mut s = shared.lock().unwrap();
    s.records = Arc::new(records);
    s.last_scan_ms = last;
    s.generation += 1;
    ctx.request_repaint();
}
