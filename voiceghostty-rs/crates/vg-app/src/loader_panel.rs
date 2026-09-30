//! 🧩 加载面板:技能 / Agent / 插件三个页签,来自设置里添加的文件夹,每个文件夹一组。
//! 三个页签是同一份列表的三个视图:在一页勾的在另一页也保留,"应用"一次提交全部页签。
//! 逻辑都在 `vg_core::loader`,这里只管界面。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Color32, RichText};

use vg_core::loader::{self, Blocker, Kind, LoaderState, ORPHAN_NOTE};

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

const WARN: Color32 = Color32::from_rgb(235, 150, 50);

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
        let mut out = PanelOutput::default();
        let tab = *self.tab.get_or_insert(Kind::Skill);

        let Some(state) = self.state.as_mut() else {
            ui.colored_label(WARN, "无法确定当前终端的工作目录");
            if ui.button("关闭").clicked() {
                out.close = true;
            }
            return out;
        };

        // 页签(带数量:没点开过的页签值不值得看,数字说了算)
        ui.horizontal(|ui| {
            for kind in Kind::ALL {
                let count = state.items.iter().filter(|i| i.kind == kind).count();
                let label = if count > 0 { format!("{} {count}", kind.label()) } else { kind.label().into() };
                if ui.selectable_label(tab == kind, label).clicked() {
                    self.tab = Some(kind);
                }
            }
        });

        // 当前目录 + 全部展开/收起 + 重新扫描
        let groups = groups(state, tab);
        let mut rescan = false;
        ui.horizontal(|ui| {
            let project = loader::tilde(&state.project);
            let targets = format!(
                "技能 → {p}/.claude/skills/\nAgent → {p}/.claude/agents/\n插件 → {p}/.claude/settings.local.json",
                p = state.project.display()
            );
            ui.label(RichText::new(format!("当前目录:{project}")).small().weak()).on_hover_text(targets);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⟳").on_hover_text("重新扫描").clicked() {
                    rescan = true;
                }
                if groups.len() > 1 {
                    let ids: Vec<String> = groups.iter().map(|g| g.id.clone()).collect();
                    let all_open = ids.iter().all(|id| !collapsed.contains(id));
                    let all_shut = ids.iter().all(|id| collapsed.contains(id));
                    if ui.add_enabled(!all_shut, egui::Button::new("⊟").small()).on_hover_text("全部收起").clicked() {
                        collapsed.extend(ids.iter().cloned());
                        out.collapsed_changed = true;
                    }
                    if ui.add_enabled(!all_open, egui::Button::new("⊞").small()).on_hover_text("全部展开").clicked() {
                        for id in &ids {
                            collapsed.remove(id);
                        }
                        out.collapsed_changed = true;
                    }
                }
            });
        });

        if let Some(w) = &state.warning {
            ui.colored_label(WARN, RichText::new(w).small());
        }
        let dups = state
            .items
            .iter()
            .filter(|i| i.kind == tab && matches!(i.blocker, Some(Blocker::Duplicate(_))))
            .count();
        if dups > 0 {
            ui.colored_label(WARN, RichText::new(format!("⚠ {dups} 项重名已跳过 —— 同一个名字只能由一个文件夹提供")).small());
        }

        if groups.is_empty() {
            ui.label(RichText::new(empty_message(state.scans.is_empty(), tab)).small().weak());
        } else {
            ui.separator();
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
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
                    let arrow = if is_shut { "▸" } else { "▾" };
                    let header = RichText::new(format!("{arrow} {}  {checked}/{}", group.title, members.len())).small().strong();
                    if ui.add(egui::Label::new(header).sense(egui::Sense::click())).on_hover_text(&group.tooltip).clicked() {
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
                        let item = &mut state.items[n];
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            let icon = match (&item.blocker, item.kind) {
                                (Some(_), _) => "⚠",
                                (None, Kind::Plugin) => "📦",
                                (None, Kind::Skill) => "🧩",
                                (None, Kind::Agent) => "👤",
                            };
                            let text = RichText::new(format!("{icon} {}", item.name));
                            let text = if item.blocker.is_some() { text.weak() } else { text };
                            ui.add_enabled(item.blocker.is_none(), egui::Checkbox::new(&mut item.checked, text))
                                .on_hover_text(&item.detail)
                                .on_disabled_hover_text(&item.detail);
                            let badge = match item.blocker {
                                Some(Blocker::Duplicate(_)) => Some("重名"),
                                Some(Blocker::NoMarketplace) => Some("无 marketplace"),
                                None => None,
                            };
                            if let Some(b) = badge {
                                ui.colored_label(WARN, RichText::new(b).small());
                            }
                        });
                    }
                }
            });
            ui.separator();
        }

        if !self.status.is_empty() {
            ui.label(RichText::new(&self.status).small().weak());
        }

        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let apply = ui
                    .add_enabled(state.has_changes(), egui::Button::new("应用"))
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
                if ui.button("取消").clicked() {
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
        return "还没有添加文件夹 —— 去设置里添加,里面的技能、Agent、插件会被自动识别。";
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
