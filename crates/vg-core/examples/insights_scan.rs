//! 开发用:把本机 Claude Code 记录扫进指定目录的台账,打印汇总。
//! `cargo run -p vg-core --release --example insights_scan -- <台账目录>`

use std::collections::BTreeMap;
use vg_core::insights::{claude, ledger, rules, scan, PriceTable, Totals};

fn main() {
    let dir = std::env::args().nth(1).map(std::path::PathBuf::from).unwrap_or_else(ledger::usage_dir);
    let t = std::time::Instant::now();
    let report = scan::scan(&claude::projects_root(), &dir, |_, _| {}).expect("scan");
    println!("{report:?} in {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let all = ledger::load(&dir, 0);
    println!("{} rows loaded in {:?}", all.len(), t.elapsed());
    let prices = PriceTable::default();
    let mut by_model: BTreeMap<String, Totals> = BTreeMap::new();
    let mut total = Totals::default();
    for r in &all {
        by_model.entry(r.model.clone()).or_default().add(r, &prices);
        total.add(r, &prices);
    }
    for (m, t) in &by_model {
        println!("{m:28} turns {:6} tokens {:>14} cost {:>10.2} hit {:.2}", t.turns, t.tokens(), t.cost, t.cache_hit().unwrap_or(0.0));
    }
    println!("TOTAL turns {} tokens {} cost {:.2} subagent rows {}", total.turns, total.tokens(), total.cost, all.iter().filter(|r| r.is_subagent()).count());
    for a in rules::advise(&all, &prices, &Default::default()) {
        println!("{:?} {:?} {} | {} | {:?}", a.severity, a.project, a.title, a.evidence, a.saving_cost);
    }
}
