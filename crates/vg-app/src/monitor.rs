//! 后台监视线程:每 0.5 秒推进所有窗格的状态灯、写会话记录、读 claude 用量,该响就响提示音。
//! 放在 UI 线程之外,是因为最需要提示音的时候恰恰是窗口在后台、egui 不重绘的时候。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::terminal::session::PaneShared;

/// 一个长任务可能 done→busy→done 来回闪(两段输出之间停顿),多个窗格也可能同一 tick 做完。
/// 限制为每 5 秒最多响一次,听起来是一条通知而不是一阵嘎啦声
const CHIME_MIN_INTERVAL: Duration = Duration::from_secs(5);

pub struct Monitor {
    panes: Arc<Mutex<Vec<Weak<PaneShared>>>>,
    pub chime_enabled: Arc<AtomicBool>,
}

impl Monitor {
    pub fn start(ctx: egui::Context, chime_enabled: bool) -> Monitor {
        let panes: Arc<Mutex<Vec<Weak<PaneShared>>>> = Arc::default();
        let chime_flag = Arc::new(AtomicBool::new(chime_enabled));
        let (list, flag) = (panes.clone(), chime_flag.clone());
        std::thread::Builder::new()
            .name("vg-monitor".into())
            .spawn(move || {
                let mut n: u64 = 0;
                let mut last_chime: Option<Instant> = None;
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    n += 1;
                    let alive: Vec<Arc<PaneShared>> = {
                        let mut list = list.lock().unwrap();
                        list.retain(|w| w.strong_count() > 0);
                        list.iter().filter_map(Weak::upgrade).collect()
                    };
                    let mut changed = false;
                    let mut chime = false;
                    for pane in alive {
                        let r = pane.tick(n);
                        changed |= r.changed;
                        chime |= r.chime;
                    }
                    if chime
                        && flag.load(Ordering::Relaxed)
                        && last_chime.is_none_or(|t| t.elapsed() >= CHIME_MIN_INTERVAL)
                    {
                        last_chime = Some(Instant::now());
                        vg_voice::chime::play();
                    }
                    if changed {
                        ctx.request_repaint();
                    }
                }
            })
            .expect("spawn monitor thread");
        Monitor { panes, chime_enabled: chime_flag }
    }

    pub fn watch(&self, pane: &Arc<PaneShared>) {
        self.panes.lock().unwrap().push(Arc::downgrade(pane));
    }
}

/// 清理已退出的 App 实例留下的状态文件(被强杀时来不及删)。文件名以创建它的进程号开头
pub fn clean_stale_state_files() {
    let Ok(entries) = std::fs::read_dir(vg_core::paths::run_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(pid) = name.split('-').next().and_then(|p| p.parse::<u32>().ok()) else { continue };
        if pid != std::process::id() && !process_alive(pid) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // 信号 0 只检查进程是否存在;EPERM 表示存在但属于别人
    let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    true // 保守:不确定就不删
}
