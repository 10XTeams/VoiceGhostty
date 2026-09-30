//! 小部件与辅助函数:状态灯圆点、claude 用量表、目录名、在文件管理器里打开……

use std::path::{Path, PathBuf};

use egui::{Color32, RichText};

use vg_core::claude_usage::ContextUsage;
use vg_core::{SessionStatus, Theme};

pub const WARN: Color32 = Color32::from_rgb(235, 150, 50);
pub const ERROR: Color32 = Color32::from_rgb(230, 90, 90);

pub fn apply_visuals(ctx: &egui::Context, theme: &Theme) {
    let bg = theme.background;
    let luminance = 0.299 * bg.0 as f32 + 0.587 * bg.1 as f32 + 0.114 * bg.2 as f32;
    ctx.set_visuals(if luminance > 128.0 { egui::Visuals::light() } else { egui::Visuals::dark() });
}

pub fn language_label(code: &str) -> &'static str {
    match code {
        "zh" => "中文",
        "en" => "English",
        _ => "自动检测",
    }
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).chain(std::iter::once('…')).collect()
    }
}

pub fn status_color(status: SessionStatus) -> Color32 {
    match status {
        SessionStatus::Normal => Color32::from_gray(110),
        SessionStatus::Busy => Color32::from_rgb(230, 190, 40),
        SessionStatus::Done => Color32::from_rgb(70, 200, 90),
    }
}

/// 一个标签页的状态灯:每个分屏一个圆点;分得太多画不下时退回一个汇总圆点(done > busy > normal)
pub fn status_dots(ui: &mut egui::Ui, statuses: &[SessionStatus]) {
    const MAX_DOTS: usize = 4;
    let shown: Vec<SessionStatus> = if statuses.len() > MAX_DOTS {
        vec![statuses.iter().copied().max_by_key(|s| s.priority()).unwrap_or_default()]
    } else {
        statuses.to_vec()
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(shown.len() as f32 * 9.0, 8.0), egui::Sense::hover());
    for (i, s) in shown.iter().enumerate() {
        let c = egui::pos2(rect.min.x + 4.0 + i as f32 * 9.0, rect.center().y);
        ui.painter().circle_filled(c, 4.0, status_color(*s));
    }
}

/// 当前窗格里 Claude Code 的上下文占用:进度条 + 数字。70% 起橙、90% 起红(压缩大约在 90% 发生)
pub fn claude_meter(ui: &mut egui::Ui, usage: &ContextUsage) {
    let fraction = usage.fraction() as f32;
    let color = if fraction >= 0.9 {
        Color32::from_rgb(230, 80, 80)
    } else if fraction >= 0.7 {
        Color32::from_rgb(240, 150, 50)
    } else {
        ui.visuals().weak_text_color()
    };
    let mut tip = format!(
        "上下文:{} / {} tokens({}%)\n统计到 claude 上一次请求 —— 正在进行的那次还没计入。",
        usage.tokens,
        usage.limit,
        usage.percent()
    );
    if let Some(m) = &usage.model {
        tip.push_str(&format!("\n模型:{m}"));
    }
    if let Some(s) = &usage.session_id {
        tip.push_str(&format!("\n会话:{s}"));
    }
    tip.push_str(&format!("\n{}", usage.transcript.display()));

    // right_to_left 布局:先加的在右边
    ui.label(RichText::new(format!("{} · {}%", usage.short(), usage.percent())).color(color).monospace())
        .on_hover_text(&tip);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(42.0, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, ui.visuals().faint_bg_color);
    let mut fill = rect;
    fill.set_width((42.0 * fraction).max(2.0));
    ui.painter().rect_filled(fill, 2.0, color);
    ui.label(RichText::new("claude").weak());
}

pub fn dir_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string())
}

/// 新加的文件夹里有多少项与上面的文件夹重名、无法从这里加载——添加的当下就说,别等用户去面板里发现
pub fn duplicate_notice(added: &[PathBuf], dirs: &[PathBuf]) -> Option<String> {
    let counts = vg_core::loader::duplicate_counts(dirs);
    let offenders: Vec<&PathBuf> = added.iter().filter(|d| counts.get(*d).copied().unwrap_or(0) > 0).collect();
    if offenders.is_empty() {
        return None;
    }
    let total: usize = offenders.iter().map(|d| counts[*d]).sum();
    let names: Vec<String> = offenders.iter().map(|d| dir_name(d)).collect();
    Some(format!("{}:有 {total} 项与上面的文件夹重名,无法从这里加载。", names.join(", ")))
}

pub fn open_in_file_manager(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(dir).spawn();
}
