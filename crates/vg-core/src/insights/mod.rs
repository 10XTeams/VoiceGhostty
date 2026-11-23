//! 用量洞察:把 Claude Code(之后还有 Codex)的本地会话记录整理成一份只有数字的台账,
//! 面板和省钱建议都读台账。设计见 `backlog/usage-insights.md`。
//!
//! 数据流:适配器(`claude`)解析 JSONL → 扫描(`scan`,游标 + 去重)→ 台账(`ledger`)
//! → 汇总(这里的 `Filter` / `Totals`)→ 规则(`rules`)。现有的状态栏用量条不经过这里。

pub mod claude;
pub mod ledger;
pub mod pricing;
pub mod rules;
pub mod scan;
pub mod time;

use std::collections::BTreeSet;

pub use pricing::{cost_of, vendor_of, Price, PriceTable};
pub use rules::{Advice, AdviceAction, RuleId, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub const ALL: [Agent; 2] = [Agent::Claude, Agent::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    pub fn parse(s: &str) -> Option<Agent> {
        match s {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }
}

/// 台账里的一行:一轮对话(从用户发出一句话到 agent 停下等下一句)。
/// 子代理的对话单独成行,`session` 填父会话,`subagent_id` 非空。只存数字,不存对话内容
#[derive(Debug, Clone, PartialEq)]
pub struct TurnRecord {
    /// 这一轮开始的时刻(Unix 毫秒,UTC)
    pub at_ms: i64,
    pub agent: Agent,
    /// 工作目录
    pub project: String,
    pub session: String,
    /// 同一会话(同一子代理)内的序号,从 0 开始;与 `session`、`subagent_id` 一起做去重键
    pub turn: u32,
    pub model: String,
    /// 未命中缓存的输入
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
    /// 这一轮发了几次 API 请求
    pub requests: u32,
    pub duration_ms: u64,
    pub thinking_ms: u64,
    pub tool_calls: u32,
    pub max_tool_output: u64,
    /// 等确认时长(橙灯),只有在 VoiceGhostty 窗格里跑才知道;None 显示"—"
    pub wait_ms: Option<u64>,
    /// 这一轮第一次请求带的上下文(输入 + 缓存写 + 缓存读)
    pub first_context: u64,
    /// 这一轮里最大的一次请求(上下文 + 输出)
    pub peak_context: u64,
    /// 隔了超过缓存 TTL 才发的请求重写进缓存的 token 数
    pub cache_rebuild: u64,
    pub subagent_type: String,
    pub subagent_id: String,
    /// 这一轮触发的技能,逗号分隔
    pub skills: String,
    pub skill_bytes: u64,
}

impl TurnRecord {
    pub fn cache_write(&self) -> u64 {
        self.cache_write_5m + self.cache_write_1h
    }

    /// Token 总量:四项原始数量直接相加(Claude 的 `input_tokens` 不含缓存读写,可以直接加)
    pub fn tokens(&self) -> u64 {
        self.input + self.cache_write() + self.cache_read + self.output
    }

    /// 折合费用(美元);表里没有这个模型时为 None
    pub fn cost(&self, prices: &pricing::PriceTable) -> Option<f64> {
        prices.get(&self.model).map(|p| cost_of(&p, self))
    }

    pub fn is_subagent(&self) -> bool {
        !self.subagent_id.is_empty()
    }

    /// 去重键
    pub fn key(&self) -> (Agent, String, String, u32) {
        (self.agent, self.session.clone(), self.subagent_id.clone(), self.turn)
    }
}

/// 面板的筛选条件,对所有页签同时生效
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    /// 只看这段时间(含)之后开始的轮次
    pub since_ms: i64,
    pub agent: Option<Agent>,
    /// 空 = 全部模型
    pub models: BTreeSet<String>,
    pub project: Option<String>,
}

impl Filter {
    pub fn accepts(&self, r: &TurnRecord) -> bool {
        r.at_ms >= self.since_ms
            && self.agent.is_none_or(|a| a == r.agent)
            && (self.models.is_empty() || self.models.contains(&r.model))
            && self.project.as_ref().is_none_or(|p| *p == r.project)
    }
}

/// 一组轮次的合计
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub turns: u64,
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
    /// 有单价的那部分的费用
    pub cost: f64,
    /// 有没有算不出费用的轮次(模型不在单价表里)
    pub unpriced_turns: u64,
    pub duration_ms: u64,
}

impl Totals {
    pub fn add(&mut self, r: &TurnRecord, prices: &pricing::PriceTable) {
        self.turns += 1;
        self.input += r.input;
        self.cache_write += r.cache_write();
        self.cache_read += r.cache_read;
        self.output += r.output;
        self.duration_ms += r.duration_ms;
        match r.cost(prices) {
            Some(c) => self.cost += c,
            None => self.unpriced_turns += 1,
        }
    }

    pub fn tokens(&self) -> u64 {
        self.input + self.cache_write + self.cache_read + self.output
    }

    /// 缓存命中率:缓存读 / 全部输入侧
    pub fn cache_hit(&self) -> Option<f64> {
        let all = self.input + self.cache_write + self.cache_read;
        (all > 0).then(|| self.cache_read as f64 / all as f64)
    }

    /// 全部都没有单价时显示"—"
    pub fn cost_known(&self) -> bool {
        self.unpriced_turns < self.turns
    }
}

/// 金额:小于 1 美元保留两位小数之外再多一位,方便看出差别
pub fn money(v: f64) -> String {
    if v >= 100.0 {
        format!("${v:.0}")
    } else if v >= 1.0 {
        format!("${v:.2}")
    } else {
        format!("${v:.3}")
    }
}

/// 毫秒 → "1:05:03" / "4:12" / "12s"
pub fn duration_label(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        return format!("{s}s");
    }
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}
