//! 按 **本地自然日** 追加写入：`logs/monitor_YYYY-MM-DD.csv`
//! 每行 = 一次验证通过的套利；含 UTC/本地时间、`cycle_id`、全量指标与两侧订单簿前 5 档（JSON）。无周期汇总行。

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use csv::WriterBuilder;

use crate::arbitrage_detector::ArbitrageOpportunity;

const CSV_HEADER: &[&str] = &[
    "event_time_utc_rfc3339",
    "event_time_local",
    "cycle_id",
    "cycle_phase",
    "pm_market_id",
    "kalshi_market_id",
    "pm_title",
    "kalshi_title",
    "text_similarity",
    "match_pm_side",
    "match_kalshi_side",
    "needs_inversion",
    "pm_resolution_utc",
    "kalshi_resolution_utc",
    "strategy",
    "pm_action_verb",
    "pm_action_outcome",
    "pm_action_price",
    "kalshi_action_verb",
    "kalshi_action_outcome",
    "kalshi_action_price",
    "total_cost",
    "gross_profit",
    "fees_simple",
    "net_profit_simple",
    "roi_percent_simple",
    "gas_fee_field",
    "final_profit_field",
    "final_roi_field",
    "pm_optimal",
    "kalshi_optimal",
    "pm_avg_slipped",
    "kalshi_avg_slipped",
    "contracts_n",
    "capital_used",
    "fees_amount",
    "gas_amount",
    "net_profit_100",
    "roi_100_percent",
    "orderbook_pm_top5_json",
    "orderbook_kalshi_top5_json",
];

pub struct MonitorLogger {
    logs_dir: String,
    lock: Mutex<()>,
}

impl MonitorLogger {
    pub fn new(logs_dir: String) -> Result<Self> {
        fs::create_dir_all(&logs_dir)?;
        Ok(Self {
            logs_dir,
            lock: Mutex::new(()),
        })
    }

    fn path_for_local_date(&self, date_local: &str) -> PathBuf {
        Path::new(&self.logs_dir).join(format!("monitor_{}.csv", date_local))
    }

    fn local_date_string() -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    fn append_records(&self, rows: Vec<Vec<String>>) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let date = Self::local_date_string();
        let path = self.path_for_local_date(&date);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let need_header = !path.exists() || fs::metadata(&path)?.len() == 0;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let mut w = WriterBuilder::new()
            .has_headers(false)
            .from_writer(file);

        if need_header {
            w.write_record(CSV_HEADER)?;
        }

        for row in rows {
            w.write_record(&row)?;
        }
        w.flush()?;
        Ok(())
    }

    /// 一次套利识别一行：时间与周期号在列中，订单簿为 JSON（与日志中 Top5 同源数据）。
    #[allow(clippy::too_many_arguments)]
    pub fn log_arbitrage_opportunity(
        &self,
        cycle_id: usize,
        cycle_phase: &str,
        opp: &ArbitrageOpportunity,
        pm_market_id: &str,
        kalshi_market_id: &str,
        pm_title: &str,
        kalshi_title: &str,
        similarity: f64,
        match_pm_side: &str,
        match_kalshi_side: &str,
        needs_inversion: bool,
        pm_resolution: Option<DateTime<Utc>>,
        kalshi_resolution: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let n = CSV_HEADER.len();
        let mut r = vec![String::new(); n];
        let at_utc = Utc::now();
        let local_line = at_utc
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let ob_pm = serde_json::to_string(&opp.orderbook_pm_top5).unwrap_or_default();
        let ob_ks = serde_json::to_string(&opp.orderbook_kalshi_top5).unwrap_or_default();

        r[0] = at_utc.to_rfc3339();
        r[1] = local_line;
        r[2] = cycle_id.to_string();
        r[3] = cycle_phase.into();
        r[4] = pm_market_id.into();
        r[5] = kalshi_market_id.into();
        r[6] = pm_title.into();
        r[7] = kalshi_title.into();
        r[8] = format!("{:.6}", similarity);
        r[9] = match_pm_side.into();
        r[10] = match_kalshi_side.into();
        r[11] = if needs_inversion { "true" } else { "false" }.into();
        r[12] = pm_resolution.map(|d| d.to_rfc3339()).unwrap_or_default();
        r[13] = kalshi_resolution.map(|d| d.to_rfc3339()).unwrap_or_default();
        r[14] = opp.strategy.clone();
        r[15] = opp.polymarket_action.0.clone();
        r[16] = opp.polymarket_action.1.clone();
        r[17] = fmt_f64(opp.polymarket_action.2);
        r[18] = opp.kalshi_action.0.clone();
        r[19] = opp.kalshi_action.1.clone();
        r[20] = fmt_f64(opp.kalshi_action.2);
        r[21] = fmt_f64(opp.total_cost);
        r[22] = fmt_f64(opp.gross_profit);
        r[23] = fmt_f64(opp.fees);
        r[24] = fmt_f64(opp.net_profit);
        r[25] = fmt_f64(opp.roi_percent);
        r[26] = fmt_f64(opp.gas_fee);
        r[27] = fmt_f64(opp.final_profit);
        r[28] = fmt_f64(opp.final_roi_percent);
        r[29] = fmt_f64(opp.pm_optimal);
        r[30] = fmt_f64(opp.kalshi_optimal);
        r[31] = fmt_f64(opp.pm_avg_slipped);
        r[32] = fmt_f64(opp.kalshi_avg_slipped);
        r[33] = fmt_f64(opp.contracts);
        r[34] = fmt_f64(opp.capital_used);
        r[35] = fmt_f64(opp.fees_amount);
        r[36] = fmt_f64(opp.gas_amount);
        r[37] = fmt_f64(opp.net_profit_100);
        r[38] = fmt_f64(opp.roi_100_percent);
        r[39] = ob_pm;
        r[40] = ob_ks;

        debug_assert_eq!(r.len(), n);
        self.append_records(vec![r])
    }
}

fn fmt_f64(x: f64) -> String {
    format!("{:.12}", x)
}
