//! 全量/追踪周期外的**纯统计**：累计套利次数与资金汇总，不改变套利判定逻辑。
//!
//! - **GLOBAL**：自进程启动以来累计（不重置）。
//! - **BIG_PERIOD**：当前大周期内累计（1 次全量匹配 + 其后 interval-1 次价格追踪）；在**下一全量匹配开始前**结帐并清零。
use std::cmp::Ordering;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;

use crate::arbitrage_detector::ArbitrageOpportunity;

/// 与 `main.rs` 中 `OpportunityRow` 一致的元组，便于传入本模块而不依赖 main 私有别名。
pub type OpportunityTuple = (
    ArbitrageOpportunity,
    String,
    String,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

#[derive(Debug, Default, Clone)]
struct CumulativeStats {
    /// 已成功识别为套利的次数（全量+追踪，每次验证通过计 1，重复市场对分开计）
    arb_hits: u64,
    sum_capital: f64,
    sum_gas: f64,
    sum_fees: f64,
    /// 兑付面额合计（每份合约 $1 计）
    sum_gross_payout: f64,
    sum_net_profit: f64,
    /// 已完成的「全量匹配」周期个数
    full_match_cycles_completed: u64,
}

/// 当前大周期内（自上次结帐后的全量匹配起，含该次全量及其后追踪）的累计，结帐后清零。
#[derive(Debug, Default, Clone)]
struct BigPeriodStats {
    arb_hits: u64,
    sum_capital: f64,
    sum_gas: f64,
    sum_fees: f64,
    sum_gross_payout: f64,
    sum_net_profit: f64,
}

static GLOBAL: Lazy<Mutex<CumulativeStats>> = Lazy::new(|| Mutex::new(CumulativeStats::default()));
static BIG_PERIOD: Lazy<Mutex<BigPeriodStats>> = Lazy::new(|| Mutex::new(BigPeriodStats::default()));

/// 每次 `validate_arbitrage_pair` 成功并生成 `ArbitrageOpportunity` 时调用（全量/追踪均适用）。
pub fn record_opportunity(opp: &ArbitrageOpportunity) {
    let mut g = GLOBAL.lock().unwrap();
    g.arb_hits += 1;
    g.sum_capital += opp.capital_used;
    g.sum_gas += opp.gas_amount;
    g.sum_fees += opp.fees_amount;
    g.sum_gross_payout += opp.contracts;
    g.sum_net_profit += opp.net_profit_100;
    drop(g);

    let mut bp = BIG_PERIOD.lock().unwrap();
    bp.arb_hits += 1;
    bp.sum_capital += opp.capital_used;
    bp.sum_gas += opp.gas_amount;
    bp.sum_fees += opp.fees_amount;
    bp.sum_gross_payout += opp.contracts;
    bp.sum_net_profit += opp.net_profit_100;
}

/// 在**下一全量匹配周期开始前**调用（`current_cycle > 0` 且 `should_full_match()`）。
/// 输出「上一大周期」总绩效并清零大周期累加器；同时附带自启动以来累计快照。
/// 返回与终端一致的文本，供写入 `cycle_report`。
pub fn flush_big_period_report_at_boundary(current_cycle: usize, interval: usize) -> String {
    use std::fmt::Write as _;

    let ended_period_no = current_cycle / interval;
    let n_track = interval.saturating_sub(1);

    let bp = std::mem::take(&mut *BIG_PERIOD.lock().unwrap());
    let g = GLOBAL.lock().unwrap().clone();

    let bp_margin = if bp.sum_capital > f64::EPSILON {
        bp.sum_net_profit / bp.sum_capital * 100.0
    } else {
        0.0
    };
    let global_margin = if g.sum_capital > f64::EPSILON {
        g.sum_net_profit / g.sum_capital * 100.0
    } else {
        0.0
    };

    let mut out = String::new();
    writeln!(out).unwrap();
    writeln!(
        out,
        "╔══════════════════════════════════════════════════════════════════════╗"
    )
    .unwrap();
    writeln!(
        out,
        "║  📊 上一大周期总绩效（大周期 #{} 已结束 · 1 次全量匹配 + {} 次价格追踪）     ║",
        ended_period_no, n_track
    )
    .unwrap();
    writeln!(
        out,
        "╚══════════════════════════════════════════════════════════════════════╝"
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "┌─ 本大周期内累计（结帐于下一匹配周期开始前）────────────────────────────"
    )
    .unwrap();
    writeln!(out, "│  套利识别次数:               {}", bp.arb_hits).unwrap();
    writeln!(out, "│  总本金 Σcapital:           ${:.2}", bp.sum_capital).unwrap();
    writeln!(out, "│  总 Gas Σgas:               ${:.2}", bp.sum_gas).unwrap();
    writeln!(out, "│  总手续费 Σfees:            ${:.2}", bp.sum_fees).unwrap();
    writeln!(out, "│  总回报(兑付额 Σn):         ${:.2}", bp.sum_gross_payout).unwrap();
    writeln!(out, "│  总净利润 Σnet:             ${:.2}", bp.sum_net_profit).unwrap();
    writeln!(
        out,
        "│  本大周期利润率 (Σnet/Σcapital): {:.2}%",
        bp_margin
    )
    .unwrap();
    writeln!(
        out,
        "└────────────────────────────────────────────────────────────────────────"
    )
    .unwrap();

    writeln!(out).unwrap();
    writeln!(
        out,
        "┌─ 自进程启动以来累计（每次识别均独立计数，含全量+追踪周期）────────────────"
    )
    .unwrap();
    writeln!(out, "│  已完成全量匹配周期数 N:     {}", g.full_match_cycles_completed).unwrap();
    writeln!(out, "│  套利识别总次数:             {}", g.arb_hits).unwrap();
    writeln!(out, "│  总成本 Σcapital:           ${:.2}", g.sum_capital).unwrap();
    writeln!(out, "│  总 Gas Σgas:               ${:.2}", g.sum_gas).unwrap();
    writeln!(out, "│  总手续费 Σfees:            ${:.2}", g.sum_fees).unwrap();
    writeln!(out, "│  总回报(兑付额 Σn):         ${:.2}", g.sum_gross_payout).unwrap();
    writeln!(out, "│  总净利润 Σnet:             ${:.2}", g.sum_net_profit).unwrap();
    writeln!(
        out,
        "│  整体利润率 (Σnet/Σcapital): {:.2}%",
        global_margin
    )
    .unwrap();
    writeln!(
        out,
        "└────────────────────────────────────────────────────────────────────────"
    )
    .unwrap();
    writeln!(out).unwrap();

    out
}

/// 每个 **全量匹配周期** 在验证与 ROI Top10 打印完成后调用（**不再**在此处输出自启动累计，改到大周期边界）。
pub fn on_full_cycle_completed(rows: &[OpportunityTuple]) -> String {
    {
        let mut g = GLOBAL.lock().unwrap();
        g.full_match_cycles_completed += 1;
    }
    let s = format_full_cycle_roi_top10_only(rows);
    print!("{}", s);
    s
}

/// 全量周期 ROI Top 10（按 ROI%），不含自启动累计块。
pub fn format_full_cycle_roi_top10_only(rows: &[OpportunityTuple]) -> String {
    use std::fmt::Write as _;

    let g = GLOBAL.lock().unwrap().clone();
    let n = g.full_match_cycles_completed;

    let mut out = String::new();
    writeln!(out).unwrap();
    writeln!(
        out,
        "╔══════════════════════════════════════════════════════════════════════╗"
    )
    .unwrap();
    writeln!(
        out,
        "║  📈 全量匹配周期 #{} · 利润率 Top 10（按 ROI%，100 USDT 腿资金口径）      ║",
        n
    )
    .unwrap();
    writeln!(
        out,
        "╚══════════════════════════════════════════════════════════════════════╝"
    )
    .unwrap();

    if rows.is_empty() {
        writeln!(out, "   （本全量周期无验证通过的套利）").unwrap();
    } else {
        let mut sorted: Vec<_> = rows.iter().collect();
        sorted.sort_by(|a, b| {
            b.0.roi_100_percent
                .partial_cmp(&a.0.roi_100_percent)
                .unwrap_or(Ordering::Equal)
        });
        for (i, (opp, pm_title, ks_title, _, _)) in sorted.iter().take(10).enumerate() {
            writeln!(
                out,
                "\n   #{:>2}  ROI {:>7.2}%  净利 ${:<10.2}  | PM: {} …",
                i + 1,
                opp.roi_100_percent,
                opp.net_profit_100,
                truncate_title(pm_title, 72)
            )
            .unwrap();
            writeln!(out, "        Kalshi: {}", truncate_title(ks_title, 76)).unwrap();
        }
    }
    writeln!(out).unwrap();
    out
}

fn truncate_title(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let t: String = s.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{}...", t)
}
