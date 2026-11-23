//! 单价表与厂家归类。
//!
//! 内置单价对照的是 Anthropic 官方价目(Claude API 技能的模型表与缓存定价说明,缓存于 2026-11-03):
//! 缓存写 5 分钟 = 1.25 × 输入、1 小时 = 2 × 输入;缓存读各模型不同,官方单列了的按单列值,
//! 没单列的按通用的 0.1 × 输入。Codex 的模型官方价目还没核对,先不内置,费用显示"—"。
//! 用户可以在设置里按模型覆盖(`insights-prices`)。
//!
//! 订阅套餐不按 token 计费,这里算出来的只是"折合费用",用来比较轻重。

use std::collections::BTreeMap;

use super::TurnRecord;

/// 每百万 token 的美元单价
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
    pub output: f64,
}

impl Price {
    /// 按官方的倍率由输入、缓存读、输出三项推出缓存写
    const fn anthropic(input: f64, cache_read: f64, output: f64) -> Price {
        Price { input, cache_write_5m: input * 1.25, cache_write_1h: input * 2.0, cache_read, output }
    }

    /// 设置里的写法:[输入, 缓存写 5 分钟, 缓存写 1 小时, 缓存读, 输出]
    pub fn from_array(a: [f64; 5]) -> Price {
        Price { input: a[0], cache_write_5m: a[1], cache_write_1h: a[2], cache_read: a[3], output: a[4] }
    }
}

/// (模型 id 前缀, 单价)。按前缀匹配,长的在前,这样 `claude-opus-5-5` 不会被 `claude-opus-5` 吃掉
const BUILTIN: &[(&str, Price)] = &[
    ("claude-fable-5-2", Price::anthropic(8.0, 0.20, 40.0)),
    ("claude-mythos-5-2", Price::anthropic(8.0, 0.20, 40.0)),
    ("claude-fable-5-1", Price::anthropic(10.0, 0.25, 50.0)),
    ("claude-mythos-5-1", Price::anthropic(10.0, 0.25, 50.0)),
    ("claude-fable-5", Price::anthropic(10.0, 1.0, 50.0)),
    ("claude-mythos-5", Price::anthropic(10.0, 1.0, 50.0)),
    ("claude-opus-5-5", Price::anthropic(4.0, 0.20, 20.0)),
    ("claude-opus-5", Price::anthropic(5.0, 0.50, 25.0)),
    ("claude-opus-4-8", Price::anthropic(5.0, 0.50, 25.0)),
    ("claude-opus-4-7", Price::anthropic(5.0, 0.50, 25.0)),
    ("claude-opus-4-6", Price::anthropic(5.0, 0.50, 25.0)),
    ("claude-sonnet-5-5", Price::anthropic(2.0, 0.10, 10.0)),
    ("claude-sonnet-5", Price::anthropic(2.0, 0.20, 10.0)),
    ("claude-sonnet-4-6", Price::anthropic(3.0, 0.30, 15.0)),
    // Haiku 5.5:提示超过 100K 时单价是 $0.50 / $2.50,按轮次记账分不出来,这里按 100K 以内计
    ("claude-haiku-5-5", Price::anthropic(0.10, 0.01, 0.50)),
    ("claude-haiku-4-5", Price::anthropic(1.0, 0.10, 5.0)),
];

/// 内置单价 + 用户覆盖
#[derive(Debug, Clone, Default)]
pub struct PriceTable {
    overrides: BTreeMap<String, Price>,
}

impl PriceTable {
    pub fn with_overrides(overrides: &BTreeMap<String, [f64; 5]>) -> PriceTable {
        PriceTable { overrides: overrides.iter().map(|(k, v)| (normalize(k), Price::from_array(*v))).collect() }
    }

    pub fn get(&self, model: &str) -> Option<Price> {
        let id = normalize(model);
        if let Some(p) = self.overrides.get(&id) {
            return Some(*p);
        }
        BUILTIN.iter().find(|(prefix, _)| id == *prefix || id.starts_with(&format!("{prefix}-"))).map(|(_, p)| *p)
    }
}

/// 去掉 `[1m]` 这类后缀、统一小写
fn normalize(model: &str) -> String {
    model.split('[').next().unwrap_or(model).trim().to_ascii_lowercase()
}

pub fn cost_of(p: &Price, r: &TurnRecord) -> f64 {
    (r.input as f64 * p.input
        + r.cache_write_5m as f64 * p.cache_write_5m
        + r.cache_write_1h as f64 * p.cache_write_1h
        + r.cache_read as f64 * p.cache_read
        + r.output as f64 * p.output)
        / 1_000_000.0
}

/// 厂家归类,筛选条和模型表按它分组;认不出的归到"其他"
pub fn vendor_of(model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude") {
        "Anthropic"
    } else if m.starts_with("gpt") || m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") || m.starts_with("codex") {
        "OpenAI"
    } else if m.starts_with("gemini") {
        "Google"
    } else {
        "其他"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_wins_and_suffixes_are_ignored() {
        let t = PriceTable::default();
        assert_eq!(t.get("claude-opus-5-5").unwrap().input, 4.0);
        assert_eq!(t.get("claude-opus-5-5[1m]").unwrap().input, 4.0);
        assert_eq!(t.get("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(t.get("claude-opus-5-20260101").unwrap().input, 5.0);
        assert_eq!(t.get("claude-fable-5-2").unwrap().cache_write_1h, 16.0);
        assert!(t.get("<synthetic>").is_none());
        assert!(t.get("gpt-5.5").is_none());
    }

    #[test]
    fn overrides_take_precedence() {
        let mut o = BTreeMap::new();
        o.insert("gpt-5.5".to_string(), [1.0, 0.0, 0.0, 0.1, 8.0]);
        o.insert("claude-opus-5-5".to_string(), [1.0, 1.0, 1.0, 1.0, 1.0]);
        let t = PriceTable::with_overrides(&o);
        assert_eq!(t.get("gpt-5.5").unwrap().output, 8.0);
        assert_eq!(t.get("claude-opus-5-5[1m]").unwrap().output, 1.0);
    }

    #[test]
    fn groups_vendors() {
        assert_eq!(vendor_of("claude-opus-5-5"), "Anthropic");
        assert_eq!(vendor_of("gpt-5.5-codex"), "OpenAI");
        assert_eq!(vendor_of("codex-auto-review"), "OpenAI");
        assert_eq!(vendor_of("llama"), "其他");
    }
}
