//! 🧩 加载面板:技能 / Agent / 插件三个页签,来自设置里添加的文件夹,每个文件夹一组。
//! 三个页签是同一份列表的三个视图:在一页勾的在另一页也保留,"应用"一次提交全部页签。
//! 逻辑都在 `vg_core::loader`,这里只管界面。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};

use vg_core::loader::{self, Blocker, Kind, LoaderState, ORPHAN_NOTE};

use crate::icons::{self, Icon};
use crate::kit;
use crate::style::{self, alpha, palette, Palette};

#[derive(Default)]
pub struct LoaderPanel {
    state: Option<LoaderState>,
    tab: Option<Kind>,
    status: String,
}

/// 面板这一帧要 App 做的事
#[derive(Default)]
pub struct PanelOutput {
    pub close: bool,
    /// 应用成功后要在终端里依次执行的重载命令
    pub reloads: Vec<&'static str>,
    /// 折叠状态变了,需要保存设置
    pub collapsed_changed: bool,
}

impl LoaderPanel {
    pub fn open(&mut self, project: Option<&Path>, dirs: &[PathBuf]) {
        self.status.clear();
        self.reload(project, dirs);
    }

    fn reload(&mut self, project: Option<&Path>, dirs: &[PathBuf]) {
        self.state = project.map(|p| LoaderState::load(p, dirs));
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        project: Option<&Path>,
        dirs: &[PathBuf],
        collapsed: &mut BTreeSet<String>,
    ) -> PanelOutput {
        let p = palette(ui.ctx());
        let mut out = PanelOutput::default();
        let tab = *self.tab.get_or_insert(Kind::Skill);

        let Some(state) = self.state.as_mut() else {
            notice(ui, &p, "无法确定当前终端的工作目录");
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::button(ui, "关闭", kit::Kind::Secondary, true).clicked() {
                    out.close = true;
                }
            });
            return out;
        };

        // 页签(带数量:没点开过的页签值不值得看,数字说了算)
        let labels: Vec<String> = Kind::ALL
            .iter()
            .map(|&kind| {
                let count = state.items.iter().filter(|i| i.kind == kind).count();
                if count > 0 { format!("{}  {count}", kind.label()) } else { kind.label().into() }
            })
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let current = Kind::ALL.iter().position(|k| *k == tab).unwrap_or(0);
        if let Some(i) = kit::segmented(ui, &refs, current) {
            self.tab = Some(Kind::ALL[i]);
        }
        ui.add_space(4.0);

        // 当前目录 + 全部展开/收起 + 重新扫描
        let groups = groups(state, tab);
        let mut rescan = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 24.0), Sense::hover());
            icons::paint(ui.painter(), r, 13.0, Icon::Folder, p.text3);
            let targets = format!(
                "技能 → {p}/.claude/skills/\nAgent → {p}/.claude/agents/\n插件 → {p}/.claude/settings.local.json",
                p = state.project.display()
            );
            ui.label(RichText::new(loader::tilde(&state.project)).font(style::small()).color(p.text2)).on_hover_text(targets);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if kit::icon_button_sized(ui, Icon::Refresh, false, 24.0, 13.0).on_hover_text("重新扫描").clicked() {
                    rescan = true;
                }
                if groups.len() > 1 {
                    let ids: Vec<String> = groups.iter().map(|g| g.id.clone()).collect();
                    let all_open = ids.iter().all(|id| !collapsed.contains(id));
                    if all_open {
                        if kit::icon_button_sized(ui, Icon::Collapse, false, 24.0, 13.0).on_hover_text("全部收起").clicked() {
                            collapsed.extend(ids.iter().cloned());
                            out.collapsed_changed = true;
                        }
                    } else if kit::icon_button_sized(ui, Icon::Expand, false, 24.0, 13.0).on_hover_text("全部展开").clicked() {
                        for id in &ids {
                            collapsed.remove(id);
                        }
                        out.collapsed_changed = true;
                    }
                }
            });
        });

        if let Some(w) = &state.warning {
            notice(ui, &p, w);
        }
        let dups = state
            .items
            .iter()
            .filter(|i| i.kind == tab && matches!(i.blocker, Some(Blocker::Duplicate(_))))
            .count();
        if dups > 0 {
            notice(ui, &p, &format!("{dups} 项重名已跳过:同一个名字只能由一个文件夹提供"));
        }

        ui.add_space(4.0);
        if groups.is_empty() {
            kit::card(ui, |ui| {
                ui.label(RichText::new(empty_message(state.scans.is_empty(), tab)).color(p.text2));
            });
        } else {
            egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for group in &groups {
                    let members: Vec<usize> = state
                        .items
                        .iter()
                        .enumerate()
                        .filter(|(_, i)| i.kind == tab && i.source_dir == group.dir)
                        .map(|(n, _)| n)
                        .collect();
                    let checked = members.iter().filter(|&&n| state.items[n].checked).count();
                    let is_shut = collapsed.contains(&group.id);

                    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(row, 6.0, alpha(p.hover, 0.6));
                    }
                    let chevron = Rect::from_center_size(pos2(row.min.x + 10.0, row.center().y), vec2(14.0, 14.0));
                    icons::paint(ui.painter(), chevron, 11.0, if is_shut { Icon::ChevronRight } else { Icon::ChevronDown }, p.text3);
                    ui.painter().text(pos2(row.min.x + 22.0, row.center().y), Align2::LEFT_CENTER, &group.title, crate::fonts::semibold(12.0), p.text2);
                    ui.painter().text(
                        pos2(row.max.x - 8.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        format!("{checked} / {}", members.len()),
                        crate::fonts::medium(11.0),
                        p.text3,
                    );
                    if resp.on_hover_text(&group.tooltip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        if is_shut {
                            collapsed.remove(&group.id);
                        } else {
                            collapsed.insert(group.id.clone());
                        }
                        out.collapsed_changed = true;
                    }
                    if is_shut {
                        continue;
                    }
                    for n in members {
                        item_row(ui, &p, &mut state.items[n]);
                    }
                    ui.add_space(6.0);
                }
            });
        }

        ui.add_space(8.0);
        kit::hairline(ui);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).font(style::small()).color(p.text2));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let apply = kit::button(ui, "应用", kit::Kind::Primary, state.has_changes())
                    .on_hover_text("三个页签的改动一起应用,不只是当前这一页");
                if apply.clicked() {
                    let outcome = state.apply();
                    self.status = outcome.summary();
                    if outcome.failures.is_empty() && !outcome.reloads.is_empty() {
                        self.status.push_str(&format!(",已发送 {}", outcome.reloads.join(" + ")));
                        out.reloads = outcome.reloads;
                    }
                    rescan = true; // 以磁盘上的实际状态刷新勾选
                }
                if kit::button(ui, "取消", kit::Kind::Secondary, true).clicked() {
                    out.close = true;
                }
            });
        });

        if rescan {
            self.reload(project, dirs);
        }
        out
    }
}

/// 一项:勾选框 + 名字 + 无法加载的原因(徽标)。整行可点
fn item_row(ui: &mut egui::Ui, p: &Palette, item: &mut loader::LoaderItem) {
    let enabled = item.blocker.is_none();
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), sense);
    if resp.hovered() && enabled {
        ui.painter().rect_filled(row, 6.0, p.hover);
    }
    if resp.clicked() {
        item.checked = !item.checked;
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, item.checked, 0.1);
    let bx = Rect::from_center_size(pos2(row.min.x + 32.0, row.center().y), vec2(16.0, 16.0));
    if enabled {
        ui.painter().rect_filled(bx, 4.0, style::mix(p.field, p.accent, t));
        if t < 1.0 {
            ui.painter().rect_stroke(bx, 4.0, Stroke::new(1.0, alpha(p.text3, 1.0 - t)), egui::StrokeKind::Inside);
        }
        if item.checked {
            let ink = if p.dark { Color32::from_rgb(8, 22, 34) } else { Color32::WHITE };
            icons::paint(ui.painter(), bx, 12.0, Icon::Check, alpha(ink, t));
        }
    } else {
        ui.painter().rect_stroke(bx, 4.0, Stroke::new(1.0, alpha(p.text3, 0.5)), egui::StrokeKind::Inside);
    }
    let color = if enabled { p.text } else { p.text3 };
    let name_end = ui.painter().text(pos2(row.min.x + 50.0, row.center().y), Align2::LEFT_CENTER, &item.name, style::body(), color).max.x;
    let badge = match item.blocker {
        Some(Blocker::Duplicate(_)) => Some("重名"),
        Some(Blocker::NoMarketplace) => Some("缺少 marketplace"),
        None => None,
    };
    if let Some(b) = badge {
        let font = crate::fonts::medium(10.5);
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(b.into(), font, p.warn));
        let r = Rect::from_min_size(pos2(name_end + 8.0, row.center().y - 9.0), galley.size() + vec2(12.0, 0.0)).with_max_y(row.center().y + 9.0);
        ui.painter().rect_filled(r, 9.0, alpha(p.warn, 0.16));
        ui.painter().galley(pos2(r.min.x + 6.0, r.center().y - galley.size().y / 2.0), galley, p.warn);
    }
    resp.on_hover_text(&item.detail);
}

fn notice(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
        icons::paint(ui.painter(), r, 12.0, Icon::Alert, p.warn);
        ui.label(RichText::new(text).font(style::small()).color(p.warn));
    });
}

struct Group {
    /// "<页签>/<文件夹>":同一个文件夹在不同页签里各自折叠
    id: String,
    dir: Option<PathBuf>,
    title: String,
    tooltip: String,
}

fn groups(state: &LoaderState, tab: Kind) -> Vec<Group> {
    let mine: Vec<_> = state.items.iter().filter(|i| i.kind == tab).collect();
    let mut result: Vec<Group> = state
        .scans
        .iter()
        .filter(|s| mine.iter().any(|i| i.source_dir.as_deref() == Some(s.dir.as_path())))
        .map(|s| Group {
            id: format!("{}/{}", tab.as_str(), s.dir.display()),
            dir: Some(s.dir.clone()),
            title: state.folder_title(&s.dir),
            tooltip: s.dir.display().to_string(),
        })
        .collect();
    if mine.iter().any(|i| i.source_dir.is_none()) {
        result.push(Group { id: format!("{}/", tab.as_str()), dir: None, title: "仅项目内".into(), tooltip: ORPHAN_NOTE.into() });
    }
    result
}

/// 每一类靠不同的标记识别,笼统一句"这里没有"会让人猜不出自己的文件夹缺了哪个
fn empty_message(no_folders: bool, tab: Kind) -> &'static str {
    if no_folders {
        return "还没有添加文件夹。去设置里添加,里面的技能、Agent、插件会被自动识别。";
    }
    match tab {
        Kind::Skill => "已添加的文件夹里没有识别到技能。技能是带 SKILL.md 的文件夹,可以在添加的文件夹本层,也可以在它的 skills/ 下 —— 插件文件夹不在这里扫描,它带的技能随插件一起加载。",
        Kind::Agent => "已添加的文件夹里没有识别到 Agent。Agent 是以 YAML front matter 开头的 .md 文件,可以在添加的文件夹本层,也可以在它的 agents/ 下 —— 插件文件夹不在这里扫描,它带的 Agent 随插件一起加载。",
        Kind::Plugin => "已添加的文件夹里没有识别到插件。插件文件夹带 .claude-plugin/marketplace.json(或 plugin.json)。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(panel: &mut LoaderPanel, project: Option<&Path>, dirs: &[PathBuf], collapsed: &mut BTreeSet<String>) -> PanelOutput {
        let ctx = egui::Context::default();
        // 和 App 一样先装字体:面板用到了自定义字重(FontFamily::Name),没装会 panic
        crate::fonts::install(&ctx, "");
        crate::style::apply(&ctx, &vg_core::Theme::DARK);
        let mut out = None;
        let mut full = ctx.run_ui(egui::RawInput::default(), |ui| {
            out = Some(panel.show(ui, project, dirs, collapsed));
        });
        full.textures_delta.clear();
        out.unwrap()
    }

    #[test]
    fn renders_every_tab_and_state() {
        let d = tempfile::tempdir().unwrap();
        let (lib, proj) = (d.path().join("lib"), d.path().join("proj"));
        std::fs::create_dir_all(lib.join("s1")).unwrap();
        std::fs::write(lib.join("s1/SKILL.md"), "").unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        let dirs = vec![lib, d.path().join("missing")];
        let mut collapsed = BTreeSet::new();

        let mut panel = LoaderPanel::default();
        panel.open(Some(&proj), &dirs);
        assert!(panel.state.as_ref().unwrap().warning.as_deref().unwrap().contains("文件夹不存在"));
        for kind in Kind::ALL {
            panel.tab = Some(kind);
            let out = render(&mut panel, Some(&proj), &dirs, &mut collapsed);
            assert!(!out.close && out.reloads.is_empty());
        }
        // 没有工作目录
        let mut panel = LoaderPanel::default();
        panel.open(None, &dirs);
        render(&mut panel, None, &dirs, &mut collapsed);
    }
}
