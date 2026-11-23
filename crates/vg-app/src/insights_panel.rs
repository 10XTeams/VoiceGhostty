//! 用量洞察面板:建议 / 用量 / 会话三个页签,共用一条筛选条(Agent、模型、项目)和时间范围。
//! 数据来自 `insights.rs` 的后台线程,汇总与规则在 `vg_core::insights`,这里只管界面。
//! 图表用 painter 自绘,不引入新 crate。原型见 `backlog/insights-panel-preview.html`。

use std::collections::{BTreeMap, BTreeSet};

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke, Ui};

use vg_core::claude_usage::compact;
use vg_core::insights::{
    duration_label, money, rules, vendor_of, Advice, AdviceAction, Agent, Filter, PriceTable, RuleId, Severity, Totals,
    TurnRecord,
};

use crate::icons::{self, Icon};
use crate::insights::Snapshot;
use crate::kit;
use crate::style::{self, alpha, palette, Palette};

const DAY_MS: i64 = 86_400_000;
/// 内容区高度(超出滚动)
pub const BODY_H: f32 = 520.0;
pub const PANEL_W: f32 = 936.0;
/// 会话页最多列这么多个,再多的看筛选
const MAX_SESSIONS: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Advice,
    Usage,
    Sessions,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum SessionSort {
    #[default]
    Time,
    Tokens,
    Duration,
}

#[derive(Default)]
pub struct InsightsPanel {
    pub tab: Tab,
    days: u32,
    agent: Option<Agent>,
    models: BTreeSet<String>,
    project: Option<String>,
    chart_cost: bool,
    chart_by_model: bool,
    sort: SessionSort,
    expanded: Option<String>,
    cache: Option<(CacheKey, View)>,
}

/// 面板这一帧要 App 做的事
#[derive(Default)]
pub struct PanelOutput {
    pub close: bool,
    pub type_command: Option<&'static str>,
    pub open_loader: bool,
    pub mute: Option<RuleId>,
    pub refresh: bool,
}

#[derive(Clone, PartialEq)]
struct CacheKey {
    generation: u64,
    filter: Filter,
    muted: BTreeSet<String>,
    day0: i64,
}

impl InsightsPanel {
    /// 打开到指定页签;`project` 非空时按它筛选(点状态栏用量条进来)
    pub fn open(&mut self, tab: Tab, project: Option<String>) {
        self.tab = tab;
        if project.is_some() {
            self.project = project;
        }
        if self.days == 0 {
            self.days = 7;
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        snap: &Snapshot,
        prices: &PriceTable,
        muted: &BTreeSet<String>,
        now_ms: i64,
        tz_offset_ms: i64,
    ) -> PanelOutput {
        let p = palette(ui.ctx());
        let mut out = PanelOutput::default();
        if self.days == 0 {
            self.days = 7;
        }
        let today = (now_ms + tz_offset_ms).div_euclid(DAY_MS);
        let day0 = today - self.days as i64 + 1;
        let filter = Filter {
            since_ms: day0 * DAY_MS - tz_offset_ms,
            agent: self.agent,
            models: self.models.clone(),
            project: self.project.clone(),
        };
        let key = CacheKey { generation: snap.generation, filter: filter.clone(), muted: muted.clone(), day0 };
        if self.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let view = View::build(&snap.records, &filter, prices, muted, day0, self.days, tz_offset_ms);
            self.cache = Some((key, view));
        }
        let view = self.cache.take().expect("刚建好");

        self.toolbar(ui, &p, &view.1, snap, &mut out);
        ui.add_space(6.0);
        self.filter_bar(ui, &p, &view.1);
        ui.add_space(8.0);

        // 应用窗口矮的时候跟着收:留出标题、工具行和上下边距
        let body_h = BODY_H.min(ui.ctx().content_rect().height() - 220.0).max(160.0);
        egui::ScrollArea::vertical()
            .max_height(body_h)
            .min_scrolled_height(body_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
            if let Some((done, total)) = snap.progress {
                if snap.records.is_empty() || total > 0 {
                    notice(ui, &p, Icon::Refresh, &format!("正在整理历史记录 {done} / {total}"), p.text2);
                }
            }
            if let Some(e) = &snap.error {
                notice(ui, &p, Icon::Alert, e, p.warn);
            }
            if let Some(v) = &snap.unverified_version {
                notice(
                    ui,
                    &p,
                    Icon::Alert,
                    &format!("Claude Code {v} 比验证过的 {} 新,记录格式未验证,数字可能不全", vg_core::insights::claude::VERIFIED_VERSION),
                    p.text3,
                );
            }
            if snap.records.is_empty() && snap.progress.is_none() {
                kit::card(ui, |ui| {
                    ui.label(RichText::new("还没有用量数据。点右上角的刷新整理一次;只读本机 ~/.claude/projects 下的记录,不调 API,数据不离开本机。").color(p.text2));
                });
                return;
            }
            match self.tab {
                Tab::Advice => self.advice_tab(ui, &p, &view.1, &mut out),
                Tab::Usage => self.usage_tab(ui, &p, &view.1),
                Tab::Sessions => self.sessions_tab(ui, &p, &view.1, prices),
            }
        });
        self.cache = Some(view);
        out
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &Palette, view: &View, snap: &Snapshot, out: &mut PanelOutput) {
        ui.horizontal(|ui| {
            let advice = if view.advice.is_empty() { "建议".to_string() } else { format!("建议  {}", view.advice.len()) };
            let tabs = [advice.as_str(), "用量", "会话"];
            let current = match self.tab {
                Tab::Advice => 0,
                Tab::Usage => 1,
                Tab::Sessions => 2,
            };
            if let Some(i) = kit::segmented(ui, &tabs, current) {
                self.tab = [Tab::Advice, Tab::Usage, Tab::Sessions][i];
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let busy = snap.progress.is_some();
                let tip = match snap.last_scan_ms {
                    Some(t) => format!("重新整理\n上次整理:{}", local_time(t)),
                    None => "重新整理".into(),
                };
                if kit::icon_button_sized(ui, Icon::Refresh, busy, 24.0, 13.0).on_hover_text(tip).clicked() && !busy {
                    out.refresh = true;
                }
                let current = if self.days == 30 { 1 } else { 0 };
                if let Some(i) = kit::segmented(ui, &["7 天", "30 天"], current) {
                    self.days = if i == 1 { 30 } else { 7 };
                }
                let _ = p;
            });
        });
    }

    fn filter_bar(&mut self, ui: &mut Ui, p: &Palette, view: &View) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let agent_label = self.agent.map_or("全部 agent", |a| a.as_str());
            let before = self.agent;
            egui::ComboBox::from_id_salt("insights-agent").selected_text(agent_label).width(110.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.agent, None, "全部 agent");
                for a in Agent::ALL {
                    ui.selectable_value(&mut self.agent, Some(a), a.as_str());
                }
            });
            if self.agent != before {
                // 已勾选的模型里不属于这个 agent 的去掉
                if let Some(a) = self.agent {
                    let usable: BTreeSet<&String> = view.all_models.iter().filter(|(_, ag)| ag.contains(&a)).map(|(m, _)| m).collect();
                    self.models.retain(|m| usable.contains(m));
                }
            }

            self.model_picker(ui, p, view);

            let project_label = match &self.project {
                Some(pr) => short_project(pr),
                None => "全部项目".into(),
            };
            egui::ComboBox::from_id_salt("insights-project").selected_text(project_label).width(200.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.project, None, "全部项目");
                for pr in &view.projects {
                    ui.selectable_value(&mut self.project, Some(pr.clone()), short_project(pr)).on_hover_text(pr);
                }
            });
            let filtered = self.agent.is_some() || !self.models.is_empty() || self.project.is_some();
            if filtered && kit::icon_button_sized(ui, Icon::Close, false, 22.0, 11.0).on_hover_text("清除筛选").clicked() {
                self.agent = None;
                self.models.clear();
                self.project = None;
            }
        });
    }

    /// 两级多选:厂家 → 模型。不选即全部;勾厂家等于勾它名下全部模型
    fn model_picker(&mut self, ui: &mut Ui, p: &Palette, view: &View) {
        let label = model_filter_label(&self.models, view);
        let resp = ui.add(egui::Button::new(RichText::new(format!("{label}  ▾")).color(p.text)).min_size(vec2(150.0, 0.0)));
        egui::Popup::from_toggle_button_response(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(240.0);
                let groups = view.vendor_models(self.agent);
                if groups.is_empty() {
                    ui.label(RichText::new("这段时间没有记录").color(p.text3));
                }
                for (vendor, models) in groups {
                    let picked = models.iter().filter(|m| self.models.contains(**m)).count();
                    let mut all = picked == models.len();
                    let r = ui.add(egui::Checkbox::new(&mut all, RichText::new(vendor).strong()).indeterminate(picked > 0 && picked < models.len()));
                    if r.clicked() {
                        for m in &models {
                            if all {
                                self.models.insert((*m).clone());
                            } else {
                                self.models.remove(*m);
                            }
                        }
                    }
                    ui.indent(vendor, |ui| {
                        for m in models {
                            let mut on = self.models.contains(m);
                            if ui.checkbox(&mut on, m.as_str()).clicked() {
                                if on {
                                    self.models.insert(m.clone());
                                } else {
                                    self.models.remove(m);
                                }
                            }
                        }
                    });
                }
                if !self.models.is_empty() {
                    ui.separator();
                    if ui.button("全部模型").clicked() {
                        self.models.clear();
                    }
                }
            });
    }

    // MARK: 建议

    fn advice_tab(&mut self, ui: &mut Ui, p: &Palette, view: &View, out: &mut PanelOutput) {
        if view.advice.is_empty() {
            kit::card(ui, |ui| {
                ui.label(RichText::new("这段时间没有发现明显多花 token 的习惯。").color(p.text2));
                ui.label(RichText::new("目前检查两项:缓存过期后接着聊、起步上下文偏重。点过「不再提示」的规则不在这里显示。").font(style::small()).color(p.text3));
            });
            return;
        }
        for a in &view.advice {
            kit::card(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                    kit::dot(ui, r.center(), 4.0, severity_color(p, a.severity), false);
                    ui.label(RichText::new(&a.title).font(crate::fonts::semibold(13.5)).color(p.text));
                    if let Some(m) = &a.model {
                        tag(ui, p, m, p.text2);
                    }
                    if let Some(pr) = &a.project {
                        tag(ui, p, &short_project(pr), p.text2).on_hover_text(pr);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let saving = match a.saving_cost {
                            Some(s) => format!("约可省 {}", money(s)),
                            None => "可省 —(模型无单价)".into(),
                        };
                        ui.label(RichText::new(saving).font(crate::fonts::medium(12.0)).color(p.done));
                    });
                });
                ui.label(RichText::new(&a.evidence).color(p.text2));
                ui.label(RichText::new(&a.suggestion).color(p.text));
                ui.horizontal(|ui| {
                    match &a.action {
                        Some(AdviceAction::TypeCommand(cmd)) => {
                            if kit::button(ui, &format!("敲入 {cmd}"), kit::Kind::Primary, true)
                                .on_hover_text("只敲进当前终端,不回车")
                                .clicked()
                            {
                                out.type_command = Some(cmd);
                            }
                        }
                        Some(AdviceAction::OpenLoader) => {
                            if kit::button(ui, "打开加载器", kit::Kind::Primary, true).clicked() {
                                out.open_loader = true;
                            }
                        }
                        None => {}
                    }
                    if let Some(pr) = &a.project {
                        if self.project.as_ref() != Some(pr) && kit::button(ui, "看这个项目的会话", kit::Kind::Secondary, true).clicked() {
                            self.project = Some(pr.clone());
                            self.tab = Tab::Sessions;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Label::new(RichText::new("不再提示").font(style::small()).color(p.text3)).sense(Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("这一类建议以后都不再显示(可在设置里恢复)")
                            .clicked()
                        {
                            out.mute = Some(a.rule);
                        }
                    });
                });
            });
            ui.add_space(8.0);
        }
    }

    // MARK: 用量

    fn usage_tab(&mut self, ui: &mut Ui, p: &Palette, view: &View) {
        let t = &view.totals;
        let main_turns = view.main_turns.max(1);
        let stats = [
            ("Token 总量", compact(t.tokens()), format!("输入 {} · 缓存写 {} · 缓存读 {} · 输出 {}", compact(t.input), compact(t.cache_write), compact(t.cache_read), compact(t.output))),
            ("折合费用", if t.cost_known() { money(t.cost) } else { "—".into() }, cost_note(t)),
            ("对话轮数", view.main_turns.to_string(), format!("另有子代理 {} 段", view.subagent_rows)),
            ("平均每轮", compact(t.tokens() / main_turns), "含子代理的用量".into()),
            ("总等待时间", duration_label(view.main_duration_ms), "主对话各轮耗时之和".into()),
            ("缓存命中率", t.cache_hit().map_or("—".into(), |h| format!("{:.0}%", h * 100.0)), "缓存读 / 全部输入".into()),
        ];
        let gap = 8.0;
        let w = ((ui.available_width() - gap * 5.0) / 6.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (label, value, tip) in stats {
                stat_tile(ui, p, w, label, &value).on_hover_text(tip);
            }
        });
        ui.add_space(10.0);

        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("每日用量").font(crate::fonts::semibold(12.5)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(i) = kit::segmented(ui, &["按类型", "按模型"], self.chart_by_model as usize) {
                        self.chart_by_model = i == 1;
                    }
                    if let Some(i) = kit::segmented(ui, &["Token", "费用"], self.chart_cost as usize) {
                        self.chart_cost = i == 1;
                    }
                });
            });
            daily_chart(ui, p, view, self.chart_cost, self.chart_by_model);
        });
        ui.add_space(10.0);

        kit::card(ui, |ui| {
            ui.label(RichText::new("模型").font(crate::fonts::semibold(12.5)).color(p.text));
            let cols = [
                Col::flex("厂家 / 模型"),
                Col::num("轮数", 60.0),
                Col::num("Token", 80.0),
                Col::num("费用", 80.0),
                Col::num("费用占比", 70.0),
                Col::num("平均每轮", 80.0),
                Col::num("平均耗时", 76.0),
                Col::num("命中率", 64.0),
            ];
            header(ui, p, &cols);
            let total_cost = t.cost.max(1e-9);
            for (vendor, vt, models) in &view.vendors {
                let all_on = models.iter().all(|(m, _)| self.models.contains(m));
                let cells = model_cells(vendor, vt, total_cost);
                let r = row(ui, p, &cols, &cells, RowStyle { bold: true, selected: all_on && !self.models.is_empty(), ..RowStyle::default() });
                if r.on_hover_text("点击:勾选 / 取消整个厂家").clicked() {
                    for (m, _) in models {
                        if all_on {
                            self.models.remove(m);
                        } else {
                            self.models.insert(m.clone());
                        }
                    }
                }
                for (m, mt) in models {
                    let cells = model_cells(m, mt, total_cost);
                    let on = self.models.contains(m);
                    let r = row(ui, p, &cols, &cells, RowStyle { indent: 16.0, selected: on, ..RowStyle::default() });
                    if r.on_hover_text("点击:加入 / 移出筛选").clicked() {
                        if on {
                            self.models.remove(m);
                        } else {
                            self.models.insert(m.clone());
                        }
                    }
                }
            }
        });
        ui.add_space(10.0);

        kit::card(ui, |ui| {
            ui.label(RichText::new("项目").font(crate::fonts::semibold(12.5)).color(p.text));
            let cols = [Col::flex("项目"), Col::num("Token", 90.0), Col::num("费用", 90.0), Col::num("轮数", 70.0)];
            header(ui, p, &cols);
            for (project, pt) in &view.project_rows {
                let cells = [short_project(project), compact(pt.tokens()), cost_cell(pt), pt.turns.to_string()];
                let on = self.project.as_ref() == Some(project);
                let r = row(ui, p, &cols, &cells, RowStyle { selected: on, ..RowStyle::default() });
                if r.on_hover_text(format!("{project}\n点击:只看这个项目")).clicked() {
                    self.project = if on { None } else { Some(project.clone()) };
                }
            }
        });
    }

    // MARK: 会话

    fn sessions_tab(&mut self, ui: &mut Ui, p: &Palette, view: &View, prices: &PriceTable) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} 个会话", view.sessions.len())).color(p.text2));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current = match self.sort {
                    SessionSort::Time => 0,
                    SessionSort::Tokens => 1,
                    SessionSort::Duration => 2,
                };
                if let Some(i) = kit::segmented(ui, &["按时间", "按用量", "按耗时"], current) {
                    self.sort = [SessionSort::Time, SessionSort::Tokens, SessionSort::Duration][i];
                }
            });
        });
        ui.add_space(4.0);
        let mut order: Vec<&SessionRow> = view.sessions.iter().collect();
        match self.sort {
            SessionSort::Time => order.sort_by_key(|s| std::cmp::Reverse(s.start_ms)),
            SessionSort::Tokens => order.sort_by(|a, b| b.cost.total_cmp(&a.cost).then(b.totals.tokens().cmp(&a.totals.tokens()))),
            SessionSort::Duration => order.sort_by_key(|s| std::cmp::Reverse(s.main_duration_ms)),
        }
        let cols = [
            Col::num_left("开始", 92.0),
            Col::flex("项目"),
            Col::num_left("agent", 56.0),
            Col::num_left("模型", 150.0),
            Col::num("轮数", 46.0),
            Col::num("Token", 70.0),
            Col::num("费用", 70.0),
            Col::num("峰值上下文", 82.0),
            Col::num("耗时", 64.0),
        ];
        header(ui, p, &cols);
        for s in order.into_iter().take(MAX_SESSIONS) {
            let open = self.expanded.as_ref() == Some(&s.id);
            let cells = [
                local_time(s.start_ms),
                short_project(&s.project),
                s.agent.as_str().to_string(),
                s.models.join(", "),
                s.main_turns.to_string(),
                compact(s.totals.tokens()),
                cost_cell(&s.totals),
                compact(s.peak_context),
                duration_label(s.main_duration_ms),
            ];
            let r = row(ui, p, &cols, &cells, RowStyle { selected: open, chevron: Some(open), ..RowStyle::default() });
            if r.on_hover_text(format!("{}\n会话 {}", s.project, s.id)).clicked() {
                self.expanded = if open { None } else { Some(s.id.clone()) };
            }
            if open {
                turn_table(ui, p, view, s, prices);
            }
        }
        if view.sessions.len() > MAX_SESSIONS {
            ui.label(RichText::new(format!("只列出前 {MAX_SESSIONS} 个,用筛选缩小范围")).font(style::small()).color(p.text3));
        }
    }
}

/// 展开的会话:每轮一行,子代理附在后面
fn turn_table(ui: &mut Ui, p: &Palette, view: &View, s: &SessionRow, prices: &PriceTable) {
    let cols = [
        Col::num_left("轮", 40.0),
        Col::num_left("时间", 60.0),
        Col::flex("模型"),
        Col::num("耗时", 60.0),
        Col::num("思考", 56.0),
        Col::num("上下文增量", 80.0),
        Col::num("输出", 64.0),
        Col::num("工具", 44.0),
        Col::num("最大工具输出", 92.0),
        Col::num("等确认", 60.0),
        Col::num("费用", 64.0),
    ];
    ui.add_space(2.0);
    egui::Frame::NONE.fill(alpha(p.hover, 0.35)).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        header(ui, p, &cols);
        let mut prev_peak = 0u64;
        for &i in &s.rows {
            let r = &view.rows[i];
            let label = if r.is_subagent() {
                format!("↳ {}", if r.subagent_type.is_empty() { "子代理" } else { &r.subagent_type })
            } else {
                (r.turn + 1).to_string()
            };
            let delta = if r.is_subagent() {
                "—".into()
            } else {
                let d = r.peak_context as i64 - prev_peak as i64;
                prev_peak = r.peak_context;
                if d >= 0 { format!("+{}", compact(d as u64)) } else { format!("−{}", compact((-d) as u64)) }
            };
            let cells = [
                label,
                clock(r.at_ms),
                r.model.clone(),
                duration_label(r.duration_ms),
                if r.thinking_ms > 0 { duration_label(r.thinking_ms) } else { "—".into() },
                delta,
                compact(r.output),
                r.tool_calls.to_string(),
                if r.max_tool_output > 0 { bytes(r.max_tool_output) } else { "—".into() },
                r.wait_ms.map_or("—".into(), duration_label),
                r.cost(prices).map_or("—".into(), money),
            ];
            let dim = r.is_subagent();
            row(ui, p, &cols, &cells, RowStyle { dim, height: 24.0, ..RowStyle::default() });
        }
    });
    ui.add_space(4.0);
}

// MARK: 汇总

/// 一次筛选的全部汇总,筛选或数据变了才重算
#[derive(Default)]
struct View {
    rows: Vec<TurnRecord>,
    /// 图表第一格的本地日期(自 1970-01-01 的天数)
    day0: i64,
    totals: Totals,
    main_turns: u64,
    main_duration_ms: u64,
    subagent_rows: u64,
    /// 每天(本地日期)一格:按类型 [输入, 缓存写, 缓存读, 输出] 的 token 与费用,以及按模型
    days: Vec<Day>,
    /// 图表按模型画时的系列(费用排序,最多 5 个 + 其他)
    top_models: Vec<String>,
    vendors: Vec<(String, Totals, Vec<(String, Totals)>)>,
    project_rows: Vec<(String, Totals)>,
    sessions: Vec<SessionRow>,
    advice: Vec<Advice>,
    /// 时间范围内(不看模型、项目筛选)出现过的模型及其 agent,筛选弹层用
    all_models: BTreeMap<String, BTreeSet<Agent>>,
    projects: Vec<String>,
}

#[derive(Default, Clone)]
struct Day {
    has_data: bool,
    by_type_tokens: [f64; 4],
    by_type_cost: [f64; 4],
    by_model_tokens: BTreeMap<String, f64>,
    by_model_cost: BTreeMap<String, f64>,
}

struct SessionRow {
    id: String,
    agent: Agent,
    project: String,
    start_ms: i64,
    models: Vec<String>,
    main_turns: u64,
    totals: Totals,
    cost: f64,
    peak_context: u64,
    main_duration_ms: u64,
    /// `View::rows` 里的下标:主对话按轮次,子代理在后
    rows: Vec<usize>,
}

impl View {
    fn build(all: &[TurnRecord], filter: &Filter, prices: &PriceTable, muted: &BTreeSet<String>, day0: i64, days: u32, tz: i64) -> View {
        let mut v = View::default();
        let mut projects = BTreeSet::new();
        for r in all.iter().filter(|r| r.at_ms >= filter.since_ms && filter.agent.is_none_or(|a| a == r.agent)) {
            v.all_models.entry(r.model.clone()).or_default().insert(r.agent);
            projects.insert(r.project.clone());
        }
        v.projects = projects.into_iter().collect();
        v.rows = all.iter().filter(|r| filter.accepts(r)).cloned().collect();
        v.days = vec![Day::default(); days as usize];
        v.day0 = day0;

        let mut model_totals: BTreeMap<String, Totals> = BTreeMap::new();
        let mut project_totals: BTreeMap<String, Totals> = BTreeMap::new();
        let mut sessions: BTreeMap<(Agent, String), SessionRow> = BTreeMap::new();
        for (i, r) in v.rows.iter().enumerate() {
            v.totals.add(r, prices);
            if r.is_subagent() {
                v.subagent_rows += 1;
            } else {
                v.main_turns += 1;
                v.main_duration_ms += r.duration_ms;
            }
            model_totals.entry(r.model.clone()).or_default().add(r, prices);
            project_totals.entry(r.project.clone()).or_default().add(r, prices);

            let d = (r.at_ms + tz).div_euclid(DAY_MS) - day0;
            if let Some(day) = usize::try_from(d).ok().and_then(|d| v.days.get_mut(d)) {
                day.has_data = true;
                let parts = [r.input, r.cache_write(), r.cache_read, r.output];
                let price = prices.get(&r.model);
                for (k, n) in parts.iter().enumerate() {
                    day.by_type_tokens[k] += *n as f64;
                }
                if let Some(pr) = price {
                    let c = [
                        r.input as f64 * pr.input,
                        r.cache_write_5m as f64 * pr.cache_write_5m + r.cache_write_1h as f64 * pr.cache_write_1h,
                        r.cache_read as f64 * pr.cache_read,
                        r.output as f64 * pr.output,
                    ];
                    for (k, x) in c.iter().enumerate() {
                        day.by_type_cost[k] += x / 1_000_000.0;
                    }
                }
                *day.by_model_tokens.entry(r.model.clone()).or_default() += r.tokens() as f64;
                *day.by_model_cost.entry(r.model.clone()).or_default() += r.cost(prices).unwrap_or(0.0);
            }

            let s = sessions.entry((r.agent, r.session.clone())).or_insert_with(|| SessionRow {
                id: r.session.clone(),
                agent: r.agent,
                project: r.project.clone(),
                start_ms: r.at_ms,
                models: Vec::new(),
                main_turns: 0,
                totals: Totals::default(),
                cost: 0.0,
                peak_context: 0,
                main_duration_ms: 0,
                rows: Vec::new(),
            });
            s.start_ms = s.start_ms.min(r.at_ms);
            s.totals.add(r, prices);
            s.rows.push(i);
            if !r.is_subagent() {
                s.main_turns += 1;
                s.main_duration_ms += r.duration_ms;
                s.peak_context = s.peak_context.max(r.peak_context);
                if !s.models.contains(&r.model) {
                    s.models.push(r.model.clone());
                }
                if s.project.is_empty() {
                    s.project = r.project.clone();
                }
            }
        }
        for s in sessions.values_mut() {
            s.cost = s.totals.cost;
            let rows = &v.rows;
            s.rows.sort_by_key(|&i| (rows[i].is_subagent(), rows[i].at_ms, rows[i].turn));
        }
        v.sessions = sessions.into_values().collect();

        let mut by_cost: Vec<(&String, &Totals)> = model_totals.iter().collect();
        by_cost.sort_by(|a, b| b.1.cost.total_cmp(&a.1.cost).then(b.1.tokens().cmp(&a.1.tokens())));
        v.top_models = by_cost.iter().take(5).map(|(m, _)| (*m).clone()).collect();

        let mut vendors: BTreeMap<&'static str, (Totals, Vec<(String, Totals)>)> = BTreeMap::new();
        for (m, t) in model_totals {
            let e = vendors.entry(vendor_of(&m)).or_default();
            merge(&mut e.0, &t);
            e.1.push((m, t));
        }
        let mut vendors: Vec<(String, Totals, Vec<(String, Totals)>)> = vendors
            .into_iter()
            .map(|(name, (t, mut ms))| {
                ms.sort_by(|a, b| b.1.cost.total_cmp(&a.1.cost));
                (name.to_string(), t, ms)
            })
            .collect();
        vendors.sort_by(|a, b| b.1.cost.total_cmp(&a.1.cost));
        v.vendors = vendors;

        let mut projects: Vec<(String, Totals)> = project_totals.into_iter().collect();
        projects.sort_by(|a, b| b.1.cost.total_cmp(&a.1.cost).then(b.1.tokens().cmp(&a.1.tokens())));
        v.project_rows = projects;

        v.advice = rules::advise(&v.rows, prices, muted);
        v
    }

    /// 筛选弹层:按厂家分组的模型(选了 agent 只列它用过的)
    fn vendor_models(&self, agent: Option<Agent>) -> Vec<(&'static str, Vec<&String>)> {
        let mut g: BTreeMap<&'static str, Vec<&String>> = BTreeMap::new();
        for (m, agents) in &self.all_models {
            if agent.is_none_or(|a| agents.contains(&a)) {
                g.entry(vendor_of(m)).or_default().push(m);
            }
        }
        g.into_iter().collect()
    }
}

fn merge(into: &mut Totals, t: &Totals) {
    into.turns += t.turns;
    into.input += t.input;
    into.cache_write += t.cache_write;
    into.cache_read += t.cache_read;
    into.output += t.output;
    into.cost += t.cost;
    into.unpriced_turns += t.unpriced_turns;
    into.duration_ms += t.duration_ms;
}

/// 按钮文字:全部模型 / 单个模型名 / "<厂家> 全部" / "N 个模型"
fn model_filter_label(models: &BTreeSet<String>, view: &View) -> String {
    match models.len() {
        0 => "全部模型".into(),
        1 => models.iter().next().unwrap().clone(),
        n => {
            let vendors: BTreeSet<&str> = models.iter().map(|m| vendor_of(m)).collect();
            if vendors.len() == 1 {
                let vendor = *vendors.iter().next().unwrap();
                let all = view.all_models.keys().filter(|m| vendor_of(m) == vendor).count();
                if all == n {
                    return format!("{vendor} 全部");
                }
            }
            format!("{n} 个模型")
        }
    }
}

// MARK: 图表

/// 按模型:最多 5 个模型 + 其他
fn series_colors(p: &Palette) -> [Color32; 6] {
    [p.accent, p.done, p.warn, style::mix(p.accent, p.error, 0.6), style::mix(p.done, p.accent, 0.5), p.text3]
}

/// 按类型:输入、缓存写、缓存读、输出。缓存读通常占绝大部分,用压暗的颜色,别让它抢了别的
fn type_colors(p: &Palette) -> [Color32; 6] {
    [p.accent, p.warn, style::mix(p.text3, p.term, 0.25), p.done, p.text3, p.text3]
}

fn daily_chart(ui: &mut Ui, p: &Palette, view: &View, cost: bool, by_model: bool) {
    let colors = if by_model { series_colors(p) } else { type_colors(p) };
    let names: Vec<String> = if by_model {
        let mut n = view.top_models.clone();
        n.push("其他".into());
        n
    } else {
        ["输入", "缓存写", "缓存读", "输出"].iter().map(|s| s.to_string()).collect()
    };
    let values = |day: &Day| -> Vec<f64> {
        if by_model {
            let map = if cost { &day.by_model_cost } else { &day.by_model_tokens };
            let mut v: Vec<f64> = view.top_models.iter().map(|m| map.get(m).copied().unwrap_or(0.0)).collect();
            let rest: f64 = map.iter().filter(|(m, _)| !view.top_models.contains(m)).map(|(_, x)| x).sum();
            v.push(rest);
            v
        } else if cost {
            day.by_type_cost.to_vec()
        } else {
            day.by_type_tokens.to_vec()
        }
    };
    let fmt = |x: f64| if cost { money(x) } else { compact(x as u64) };

    // 图例
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for (i, n) in names.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (r, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, colors[i.min(colors.len() - 1)]);
                ui.label(RichText::new(n).font(style::small()).color(p.text2));
            });
        }
    });

    let h = 170.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let plot = Rect::from_min_max(pos2(rect.min.x + 52.0, rect.min.y + 6.0), pos2(rect.max.x - 4.0, rect.max.y - 20.0));
    let sums: Vec<f64> = view.days.iter().map(|d| values(d).iter().sum()).collect();
    let max = sums.iter().cloned().fold(0.0, f64::max).max(if cost { 0.01 } else { 1.0 });
    let painter = ui.painter_at(rect);
    for k in 0..=2 {
        let y = plot.max.y - plot.height() * k as f32 / 2.0;
        painter.hline(plot.x_range(), y, Stroke::new(1.0, alpha(p.border, if k == 0 { 0.9 } else { 0.45 })));
        painter.text(pos2(plot.min.x - 6.0, y), Align2::RIGHT_CENTER, fmt(max * k as f64 / 2.0), style::small(), p.text3);
    }
    let n = view.days.len().max(1);
    let slot = plot.width() / n as f32;
    let bar_w = (slot * 0.62).clamp(3.0, 36.0);
    let label_every = if n > 14 { 5 } else { 1 };
    let hover = ui.input(|i| i.pointer.hover_pos());
    for (i, day) in view.days.iter().enumerate() {
        let cx = plot.min.x + slot * (i as f32 + 0.5);
        let x0 = cx - bar_w / 2.0;
        if !day.has_data {
            // 没有记录的日期画虚线,不画成零
            painter.extend(egui::Shape::dashed_line(
                &[pos2(x0, plot.max.y - 1.5), pos2(x0 + bar_w, plot.max.y - 1.5)],
                Stroke::new(1.0, p.text3),
                3.0,
                2.0,
            ));
        } else {
            let mut y = plot.max.y;
            for (k, val) in values(day).iter().enumerate() {
                let hgt = (*val / max) as f32 * plot.height();
                if hgt <= 0.0 {
                    continue;
                }
                let r = Rect::from_min_max(pos2(x0, y - hgt), pos2(x0 + bar_w, y));
                painter.rect_filled(r, 1.5, colors[k.min(colors.len() - 1)]);
                y -= hgt;
            }
        }
        if i % label_every == 0 || i == n - 1 {
            painter.text(pos2(cx, plot.max.y + 4.0), Align2::CENTER_TOP, day_label(view, i), style::small(), p.text3);
        }
        let column = Rect::from_min_max(pos2(cx - slot / 2.0, plot.min.y), pos2(cx + slot / 2.0, plot.max.y));
        if hover.is_some_and(|h| column.contains(h)) {
            painter.rect_filled(column, 2.0, alpha(p.hover, 0.25));
            let mut tip = day_label(view, i);
            if day.has_data {
                tip.push_str(&format!("  合计 {}", fmt(sums[i])));
                for (k, val) in values(day).iter().enumerate() {
                    if *val > 0.0 {
                        tip.push_str(&format!("\n{}  {}", names[k], fmt(*val)));
                    }
                }
            } else {
                tip.push_str("  没有记录");
            }
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), ui.id().with("chart-tip"), egui::PopupAnchor::Pointer)
                .show(|ui| ui.label(tip));
        }
    }
}

/// 第 i 格的日期 "11-22"
fn day_label(view: &View, i: usize) -> String {
    let (_, m, day) = vg_core::insights::time::civil_from_days(view.day0 + i as i64);
    format!("{m:02}-{day:02}")
}

// MARK: 表格

struct Col {
    title: &'static str,
    /// 0 = 占满剩余宽度
    width: f32,
    right: bool,
}

impl Col {
    fn flex(title: &'static str) -> Col {
        Col { title, width: 0.0, right: false }
    }
    fn num(title: &'static str, width: f32) -> Col {
        Col { title, width, right: true }
    }
    fn num_left(title: &'static str, width: f32) -> Col {
        Col { title, width, right: false }
    }
}

#[derive(Clone, Copy)]
struct RowStyle {
    indent: f32,
    bold: bool,
    dim: bool,
    selected: bool,
    /// Some(展开与否):第一列前画个箭头
    chevron: Option<bool>,
    height: f32,
}

impl Default for RowStyle {
    fn default() -> Self {
        RowStyle { indent: 0.0, bold: false, dim: false, selected: false, chevron: None, height: 28.0 }
    }
}

fn layout(cols: &[Col], width: f32) -> Vec<(f32, f32)> {
    let pad = 8.0;
    let fixed: f32 = cols.iter().map(|c| c.width).sum::<f32>() + pad * cols.len() as f32;
    let flex = (width - fixed).max(80.0);
    let mut x = 0.0;
    cols.iter()
        .map(|c| {
            let w = if c.width == 0.0 { flex } else { c.width };
            let r = (x, w);
            x += w + pad;
            r
        })
        .collect()
}

fn header(ui: &mut Ui, p: &Palette, cols: &[Col]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let spans = layout(cols, rect.width());
    for (c, (x, w)) in cols.iter().zip(spans) {
        let (pos, align) = if c.right { (pos2(rect.min.x + x + w, rect.center().y), Align2::RIGHT_CENTER) } else { (pos2(rect.min.x + x, rect.center().y), Align2::LEFT_CENTER) };
        ui.painter().text(pos, align, c.title, crate::fonts::medium(11.0), p.text3);
    }
    ui.painter().hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, alpha(p.border, 0.7)));
}

fn row(ui: &mut Ui, p: &Palette, cols: &[Col], cells: &[String], st: RowStyle) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), st.height), Sense::click());
    if st.selected {
        ui.painter().rect_filled(rect, 5.0, alpha(p.accent, 0.14));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, alpha(p.hover, 0.6));
    }
    let spans = layout(cols, rect.width());
    let font = if st.bold { crate::fonts::semibold(12.0) } else { egui::FontId::proportional(12.0) };
    let color = if st.dim { p.text2 } else { p.text };
    for (i, ((c, (x, w)), text)) in cols.iter().zip(spans).zip(cells).enumerate() {
        let mut x = rect.min.x + x;
        let mut w = w;
        if i == 0 {
            if let Some(open) = st.chevron {
                let r = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0));
                icons::paint(ui.painter(), r, 10.0, if open { Icon::ChevronDown } else { Icon::ChevronRight }, p.text3);
                x += 16.0;
                w -= 16.0;
            }
            x += st.indent;
            w -= st.indent;
        }
        let galley = ui.painter().layout(text.clone(), font.clone(), color, f32::INFINITY);
        let shown = if galley.size().x > w { elide(ui, text, &font, w) } else { text.clone() };
        let (pos, align) = if c.right { (pos2(x + w, rect.center().y), Align2::RIGHT_CENTER) } else { (pos2(x, rect.center().y), Align2::LEFT_CENTER) };
        ui.painter().text(pos, align, shown, font.clone(), color);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 放不下时截断加省略号
fn elide(ui: &Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len();
    while n > 1 {
        n -= 1;
        let s: String = chars[..n].iter().chain(std::iter::once(&'…')).collect();
        if ui.painter().layout_no_wrap(s.clone(), font.clone(), Color32::WHITE).size().x <= width {
            return s;
        }
    }
    "…".into()
}

fn model_cells(name: &str, t: &Totals, total_cost: f64) -> [String; 8] {
    let turns = t.turns.max(1);
    [
        name.to_string(),
        t.turns.to_string(),
        compact(t.tokens()),
        cost_cell(t),
        if t.cost_known() { format!("{:.1}%", t.cost / total_cost * 100.0) } else { "—".into() },
        compact(t.tokens() / turns),
        duration_label(t.duration_ms / turns),
        t.cache_hit().map_or("—".into(), |h| format!("{:.0}%", h * 100.0)),
    ]
}

fn cost_cell(t: &Totals) -> String {
    if t.cost_known() { money(t.cost) } else { "—".into() }
}

fn cost_note(t: &Totals) -> String {
    let mut s = "按官方单价折算,订阅套餐不按 token 计费,只用来比较轻重".to_string();
    if t.unpriced_turns > 0 {
        s.push_str(&format!("\n{} 轮的模型没有单价,未计入", t.unpriced_turns));
    }
    s
}

// MARK: 小部件

fn stat_tile(ui: &mut Ui, p: &Palette, w: f32, label: &str, value: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 62.0), Sense::hover());
    let fill = if p.dark { style::mix(p.surface, Color32::WHITE, 0.025) } else { p.chrome };
    ui.painter().rect_filled(rect, 10.0, fill);
    ui.painter().rect_stroke(rect, 10.0, Stroke::new(1.0, alpha(p.border, 0.7)), egui::StrokeKind::Inside);
    ui.painter().text(rect.min + vec2(12.0, 12.0), Align2::LEFT_TOP, label, style::small(), p.text3);
    ui.painter().text(rect.min + vec2(12.0, 30.0), Align2::LEFT_TOP, value, crate::fonts::semibold(18.0), p.text);
    resp
}

fn tag(ui: &mut Ui, p: &Palette, text: &str, color: Color32) -> egui::Response {
    let font = crate::fonts::medium(10.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let (r, resp) = ui.allocate_exact_size(galley.size() + vec2(12.0, 6.0), Sense::hover());
    ui.painter().rect_filled(r, 9.0, alpha(p.hover, 0.9));
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, color);
    resp
}

fn notice(ui: &mut Ui, p: &Palette, icon: Icon, text: &str, color: Color32) {
    let _ = p;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
        icons::paint(ui.painter(), r, 12.0, icon, color);
        ui.label(RichText::new(text).font(style::small()).color(color));
    });
    ui.add_space(4.0);
}

fn severity_color(p: &Palette, s: Severity) -> Color32 {
    match s {
        Severity::High => p.error,
        Severity::Medium => p.warn,
        Severity::Low => p.accent,
    }
}

/// 项目路径只显示最后两段:"10XTeams/VoiceGhostty"
fn short_project(path: &str) -> String {
    let parts: Vec<&str> = path.trim_end_matches('/').rsplit('/').take(2).collect();
    match parts.as_slice() {
        [] | [""] => "(未知)".into(),
        [one] => one.to_string(),
        [last, parent] => format!("{parent}/{last}"),
        _ => path.to_string(),
    }
}

fn bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    } else if n >= 1024 {
        format!("{} KB", n / 1024)
    } else {
        format!("{n} B")
    }
}

/// 本地时间 "11-22 20:15"
fn local_time(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single().map(|t| t.format("%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn clock(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// 本地时区相对 UTC 的偏移(毫秒)
pub fn tz_offset_ms() -> i64 {
    chrono::Local::now().offset().local_minus_utc() as i64 * 1000
}

/// 截图和测试用的示例数据:9 天里 4 个模型、2 个项目,第 5 天空着
#[cfg(test)]
pub fn sample_snapshot(now_ms: i64) -> Snapshot {
    let mut rs = Vec::new();
    let models = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-2", "gpt-5.5-codex"];
    for d in 0..9i64 {
        if d == 4 {
            continue; // 留一天空着,看虚线
        }
        for (k, m) in models.iter().enumerate().take(1 + (d as usize % 4)) {
            let project = if k % 2 == 0 { "/Users/me/work/VoiceGhostty" } else { "/Users/me/work/Opera" };
            let session = format!("s{d}{k}");
            for t in 0..3 {
                rs.push(rec(now_ms - d * DAY_MS - 3_600_000 + t as i64 * 60_000, m, project, &session, t, ""));
            }
            rs.push(rec(now_ms - d * DAY_MS - 3_000_000, m, project, &session, 0, "a1"));
            rs.push(rec(now_ms - d * DAY_MS - 2_900_000, m, project, &session, 0, "a2"));
        }
    }
    Snapshot { records: std::sync::Arc::new(rs), generation: 1, last_scan_ms: Some(now_ms), ..Snapshot::default() }
}

#[cfg(test)]
fn rec(at_ms: i64, model: &str, project: &str, session: &str, turn: u32, sub: &str) -> TurnRecord {
    TurnRecord {
        at_ms,
        agent: Agent::Claude,
        project: project.into(),
        session: session.into(),
        turn,
        model: model.into(),
        input: 10,
        cache_write_5m: 0,
        cache_write_1h: 100_000,
        cache_read: 900_000,
        output: 2_000,
        requests: 3,
        duration_ms: 40_000,
        thinking_ms: 3_000,
        tool_calls: 4,
        max_tool_output: 12_000,
        wait_ms: None,
        first_context: 50_000,
        peak_context: 120_000,
        cache_rebuild: if turn == 2 { 100_000 } else { 0 },
        subagent_type: if sub.is_empty() { String::new() } else { "Explore".into() },
        subagent_id: sub.into(),
        skills: String::new(),
        skill_bytes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_groups_sessions_and_vendors() {
        let now = 1_795_349_722_726;
        let snap = sample_snapshot(now);
        let tz = 8 * 3_600_000;
        let today = (now + tz).div_euclid(DAY_MS);
        let filter = Filter { since_ms: (today - 6) * DAY_MS - tz, ..Filter::default() };
        let v = View::build(&snap.records, &filter, &PriceTable::default(), &BTreeSet::new(), today - 6, 7, tz);
        assert_eq!(v.days.len(), 7);
        assert!(v.days.iter().any(|d| !d.has_data));
        assert!(v.vendors.iter().any(|(name, _, _)| name == "OpenAI"));
        let s = &v.sessions[0];
        assert_eq!(s.main_turns, 3);
        assert!(v.rows[*s.rows.last().unwrap()].is_subagent(), "子代理排在主对话的轮次之后");
        assert_eq!(model_filter_label(&BTreeSet::new(), &v), "全部模型");
        let one_vendor: BTreeSet<String> = ["gpt-5.5-codex".to_string()].into();
        assert_eq!(model_filter_label(&one_vendor, &v), "gpt-5.5-codex");
    }

    #[test]
    fn renders_every_tab() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, "");
        crate::style::apply(&ctx, &vg_core::Theme::DARK);
        let now = 1_795_349_722_726;
        let snap = sample_snapshot(now);
        let mut panel = InsightsPanel::default();
        for tab in [Tab::Advice, Tab::Usage, Tab::Sessions] {
            panel.open(tab, None);
            let mut full = ctx.run_ui(egui::RawInput::default(), |ui| {
                panel.show(ui, &snap, &PriceTable::default(), &BTreeSet::new(), now, 0);
            });
            full.textures_delta.clear();
        }
        // 空数据
        let mut full = ctx.run_ui(egui::RawInput::default(), |ui| {
            panel.show(ui, &Snapshot::default(), &PriceTable::default(), &BTreeSet::new(), now, 0);
        });
        full.textures_delta.clear();
        assert_eq!(short_project("/a/b/c"), "b/c");
    }
}
