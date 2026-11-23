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
        SessionStatus::Normal => p.accent,
        SessionStatus::Busy => p.busy,
        SessionStatus::Done => p.done,
        SessionStatus::Waiting => p.warn,
    }
}

/// 当前窗格里 Claude Code 的上下文占用:进度条 + 数字。70% 起橙、90% 起红(压缩大约在 90% 发生)。
/// `idle`:窗格已转绿、claude 在等你回复,这时右侧显示缓存倒计时(剩 1 分钟内变橙,过期后显示"已过期")。
/// 整块可点,点了打开用量洞察的会话页
pub fn claude_meter(ui: &mut egui::Ui, usage: &ContextUsage, idle: bool, now_ms: i64) -> egui::Response {
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
    let left = usage.cache_left_ms(now_ms).filter(|_| idle);
    if let Some(left) = left {
        let ttl = if usage.cache_ttl_ms >= 3_600_000 { "1 小时" } else { "5 分钟" };
        tip.push_str(&format!(
            "\n\n⏱ 提示缓存({ttl})剩余时间。过期后下一句要按缓存写重付整段上下文,离开久了回来先 /compact 或 /clear 更省。"
        ));
        if left <= 0 {
            tip.push_str("\n已过期。");
        }
    }
    tip.push_str("\n\n点击查看这个项目的用量");

    // right_to_left 布局:先加的在右边
    let mut r = match left {
        Some(left) => {
            let (text, c) = if left <= 0 {
                ("已过期".to_string(), p.warn)
            } else {
                let s = (left / 1000) as u64;
                let text = if s >= 3600 { format!("⏱ {}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("⏱ {}:{:02}", s / 60, s % 60) };
                (text, if left <= 60_000 { p.warn } else { p.text3 })
            };
            // 倒计时每秒走一次
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000 - (now_ms.rem_euclid(1000)) as u64));
            let r = ui.label(RichText::new(text).font(egui::FontId::monospace(11.0)).color(c));
            ui.add_space(6.0);
            Some(r)
        }
        None => None,
    };
    let text_color = if fraction >= 0.7 { color } else { p.text2 };
    let pct = ui.label(RichText::new(format!("{}%", usage.percent())).font(crate::fonts::medium(11.5)).color(text_color));
    r = Some(r.map_or(pct.clone(), |r| r.union(pct)));
    let r = r.unwrap().union(ui.label(RichText::new(usage.short()).font(egui::FontId::monospace(11.0)).color(p.text3)));
    let (rect, bar) = ui.allocate_exact_size(egui::vec2(48.0, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, p.control);
    let mut fill = rect;
    fill.set_width((48.0 * fraction).max(3.0));
    ui.painter().rect_filled(fill, 2.0, color);
    let label = ui.label(RichText::new("claude").font(crate::fonts::medium(11.5)).color(p.text3));
    let whole = r.union(bar).union(label);
    let resp = ui.interact(whole.rect, ui.id().with("claude-meter"), egui::Sense::click());
    if resp.hovered() {
        // 画在文字上面会盖住字,只描个底线提示可点
        let y = whole.rect.max.y + 2.0;
        ui.painter().hline(whole.rect.x_range(), y, egui::Stroke::new(1.0, style::alpha(p.text3, 0.6)));
    }
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
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
