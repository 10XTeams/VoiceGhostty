//! 省钱建议:只用规则,不用 LLM —— 为了省 token 再花 token 不划算。纯函数,输入是筛选后的轮次。
//!
//! 第一期只有两条:缓存过期后续聊、起步上下文偏重。节省量按折合费用估算和排序
//! (token 数会被便宜的缓存读撑大,不适合用来比较)。

use std::collections::{BTreeMap, BTreeSet};

use super::pricing::PriceTable;
use super::{Agent, TurnRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RuleId {
    CacheExpired,
    HeavyStart,
}

impl RuleId {
    /// 存在设置里("不再提示")
    pub fn as_str(self) -> &'static str {
        match self {
            RuleId::CacheExpired => "cache-expired",
            RuleId::HeavyStart => "heavy-start",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdviceAction {
    /// 只敲进终端、不回车,与现有安全模型一致
    TypeCommand(&'static str),
    OpenLoader,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Advice {
    pub rule: RuleId,
    pub severity: Severity,
    pub agent: Agent,
    pub model: Option<String>,
    pub project: Option<String>,
    pub title: String,
    pub evidence: String,
    pub suggestion: String,
    /// 折合费用,排序依据;模型无单价时为 None,排在最后
    pub saving_cost: Option<f64>,
    pub action: Option<AdviceAction>,
}

/// 起步上下文低于这个数不提示(系统提示 + 工具定义本身就有这么多)
const HEAVY_START_FLOOR: u64 = 40_000;
/// 比你所有项目的中位数高出这么多才算偏重:Claude Code 自己的起步量随版本变,只和自己比
const HEAVY_START_RATIO: f64 = 1.3;

pub fn advise(records: &[TurnRecord], prices: &PriceTable, muted: &BTreeSet<String>) -> Vec<Advice> {
    let mut out = Vec::new();
    if !muted.contains(RuleId::CacheExpired.as_str()) {
        out.extend(cache_expired(records, prices));
    }
    if !muted.contains(RuleId::HeavyStart.as_str()) {
        out.extend(heavy_start(records, prices));
    }
    out.sort_by(|a, b| match (a.saving_cost, b.saving_cost) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.severity.cmp(&a.severity),
    });
    out
}

fn severity(saving: Option<f64>, tokens: u64) -> Severity {
    match saving {
        Some(s) if s >= 5.0 => Severity::High,
        Some(s) if s >= 1.0 => Severity::Medium,
        Some(_) => Severity::Low,
        None if tokens >= 5_000_000 => Severity::High,
        None if tokens >= 1_000_000 => Severity::Medium,
        None => Severity::Low,
    }
}

/// 隔了超过 TTL 才接着聊:整段上下文要按缓存写重付。可省的是"缓存写 − 缓存读"的差价
fn cache_expired(records: &[TurnRecord], prices: &PriceTable) -> Vec<Advice> {
    // 按项目汇总,建议才能落到具体的地方
    let mut by_project: BTreeMap<&str, (u64, u64, Option<f64>, BTreeSet<&str>)> = BTreeMap::new();
    for r in records.iter().filter(|r| r.agent == Agent::Claude && r.cache_rebuild > 0) {
        let e = by_project.entry(&r.project).or_insert((0, 0, Some(0.0), BTreeSet::new()));
        e.0 += 1;
        e.1 += r.cache_rebuild;
        e.3.insert(&r.model);
        e.2 = match (e.2, prices.get(&r.model)) {
            // 重建时多付的:按 1 小时写价(Claude Code 默认用 1 小时缓存)减去读价
            (Some(acc), Some(p)) => Some(acc + r.cache_rebuild as f64 * (p.cache_write_1h - p.cache_read) / 1_000_000.0),
            _ => None,
        };
    }
    by_project
        .into_iter()
        .filter(|(_, (n, tokens, _, _))| *n >= 2 || *tokens >= 100_000)
        .map(|(project, (n, tokens, saving, models))| Advice {
            rule: RuleId::CacheExpired,
            severity: severity(saving, tokens),
            agent: Agent::Claude,
            model: (models.len() == 1).then(|| models.iter().next().unwrap().to_string()),
            project: Some(project.to_string()),
            title: "缓存过期后接着聊".into(),
            evidence: format!(
                "{n} 次在缓存过期后继续对话,重新写入缓存 {} tokens",
                crate::claude_usage::compact(tokens)
            ),
            suggestion: "离开前先 /compact 压缩上下文,或者回来后换新任务时直接 /clear,别带着整段旧上下文重付一遍".into(),
            saving_cost: saving,
            action: Some(AdviceAction::TypeCommand("/compact")),
        })
        .collect()
}

/// 会话第一条请求就很重:多半是这个项目装了一堆用不到的技能 / 插件 / MCP。
/// 和你自己其他项目的起步量比,不和固定值比,只挑出明显偏重的项目
fn heavy_start(records: &[TurnRecord], prices: &PriceTable) -> Vec<Advice> {
    let starts: Vec<&TurnRecord> =
        records.iter().filter(|r| r.agent == Agent::Claude && r.turn == 0 && !r.is_subagent() && r.first_context > 0).collect();
    if starts.is_empty() {
        return Vec::new();
    }
    let baseline = median(starts.iter().map(|r| r.first_context).collect());
    let mut by_project: BTreeMap<&str, Vec<&TurnRecord>> = BTreeMap::new();
    for r in starts {
        by_project.entry(&r.project).or_default().push(r);
    }
    let mut out = Vec::new();
    for (project, starts) in by_project {
        let m = median(starts.iter().map(|r| r.first_context).collect());
        if starts.len() < 2 || m < HEAVY_START_FLOOR || (m as f64) < baseline as f64 * HEAVY_START_RATIO {
            continue;
        }
        let excess = m - baseline;
        // 每个会话第一次多写进缓存的那部分;之后每轮读缓存也要多付,这里只算第一次,偏保守
        let saving = starts.iter().try_fold(0.0, |acc, r| {
            prices.get(&r.model).map(|p| acc + excess as f64 * p.cache_write_1h / 1_000_000.0)
        });
        let models: BTreeSet<&str> = starts.iter().map(|r| r.model.as_str()).collect();
        out.push(Advice {
            rule: RuleId::HeavyStart,
            severity: severity(saving, excess * starts.len() as u64),
            agent: Agent::Claude,
            model: (models.len() == 1).then(|| models.iter().next().unwrap().to_string()),
            project: Some(project.to_string()),
            title: "起步上下文偏重".into(),
            evidence: format!(
                "{} 个会话第一条请求的上下文中位数 {} tokens,你所有项目的中位数是 {}",
                starts.len(),
                crate::claude_usage::compact(m),
                crate::claude_usage::compact(baseline)
            ),
            suggestion: "在加载器里停用这个项目用不到的技能、Agent、插件;项目里配的 MCP 服务器也是一开始就加载的".into(),
            saving_cost: saving,
            action: Some(AdviceAction::OpenLoader),
        });
    }
    out
}

fn median(mut v: Vec<u64>) -> u64 {
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(project: &str, turn: u32, first: u64, rebuild: u64) -> TurnRecord {
        TurnRecord {
            at_ms: 0,
            agent: Agent::Claude,
            project: project.into(),
            session: format!("{project}{first}"),
            turn,
            model: "claude-opus-5-5".into(),
            input: 0,
            cache_write_5m: 0,
            cache_write_1h: rebuild,
            cache_read: 0,
            output: 0,
            requests: 1,
            duration_ms: 0,
            thinking_ms: 0,
            tool_calls: 0,
            max_tool_output: 0,
            wait_ms: None,
            first_context: first,
            peak_context: first,
            cache_rebuild: rebuild,
            subagent_type: String::new(),
            subagent_id: String::new(),
            skills: String::new(),
            skill_bytes: 0,
        }
    }

    #[test]
    fn flags_cache_expiry_and_heavy_starts_sorted_by_saving() {
        let rs = vec![
            rec("/a", 3, 0, 200_000),
            rec("/a", 5, 0, 300_000),
            rec("/b", 0, 60_000, 0),
            rec("/b", 0, 70_000, 0),
            rec("/c", 0, 40_000, 0),
            rec("/c", 0, 42_000, 0),
            rec("/d", 0, 41_000, 0),
            rec("/d", 0, 43_000, 0),
        ];
        let advice = advise(&rs, &PriceTable::default(), &BTreeSet::new());
        assert_eq!(advice.len(), 2);
        assert_eq!(advice[0].rule, RuleId::CacheExpired);
        // 500K × ($8 − $0.2) / 1M = $3.9
        assert!((advice[0].saving_cost.unwrap() - 3.9).abs() < 1e-9);
        assert_eq!(advice[1].rule, RuleId::HeavyStart);
        assert_eq!(advice[1].project.as_deref(), Some("/b"));

        let muted: BTreeSet<String> = [RuleId::CacheExpired.as_str().to_string()].into();
        assert_eq!(advise(&rs, &PriceTable::default(), &muted).len(), 1);
    }
}
