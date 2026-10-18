//! 小部件与辅助函数:状态灯圆点、claude 用量表、目录名、在文件管理器里打开……

use std::path::{Path, PathBuf};

use egui::{Color32, RichText};


use vg_core::claude_usage::ContextUsage;
use vg_core::SessionStatus;

use crate::style::{self, Palette};

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

pub fn status_color_of(p: &Palette, status: SessionStatus) -> Color32 {
    match status {
        SessionStatus::Normal => p.text3,
        SessionStatus::Busy => p.busy,
        SessionStatus::Done => p.done,
    }
}

/// 当前窗格里 Claude Code 的上下文占用:进度条 + 数字。70% 起橙、90% 起红(压缩大约在 90% 发生)
pub fn claude_meter(ui: &mut egui::Ui, usage: &ContextUsage) {
    let p = style::palette(ui.ctx());
    let fraction = usage.fraction() as f32;
    let color = if fraction >= 0.9 {
        p.error
    } else if fraction >= 0.7 {
        p.warn
    } else {
        p.accent
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
    let text_color = if fraction >= 0.7 { color } else { p.text2 };
    let r = ui.label(RichText::new(format!("{}%", usage.percent())).font(crate::fonts::medium(11.5)).color(text_color));
    let r = r.union(ui.label(RichText::new(usage.short()).font(egui::FontId::monospace(11.0)).color(p.text3)));
    let (rect, bar) = ui.allocate_exact_size(egui::vec2(48.0, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, p.control);
    let mut fill = rect;
    fill.set_width((48.0 * fraction).max(3.0));
    ui.painter().rect_filled(fill, 2.0, color);
    let label = ui.label(RichText::new("claude").font(crate::fonts::medium(11.5)).color(p.text3));
    r.union(bar).union(label).on_hover_text(&tip);
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
