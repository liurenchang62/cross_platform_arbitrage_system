//! 按解析日筛选市场：剔除「有明确解析时间且晚于 horizon」的远期市场；无解析日期的保留。

use chrono::{DateTime, Duration, Utc};

use crate::market::Market;
use crate::query_params::RESOLUTION_HORIZON_DAYS;

/// 若市场有 `resolution_date` 且该时间 **严格晚于** `now + horizon_days`，则剔除。
pub fn filter_markets_by_resolution_horizon(
    markets: Vec<Market>,
    now: DateTime<Utc>,
) -> Vec<Market> {
    let horizon = Duration::days(RESOLUTION_HORIZON_DAYS);
    let cutoff = now + horizon;
    markets
        .into_iter()
        .filter(|m| match m.resolution_date {
            None => true,
            Some(dt) => dt <= cutoff,
        })
        .collect()
}

/// 任一侧有解析日且该日晚于 cutoff，则该追踪对应剔除。
pub fn tracked_pair_exceeds_horizon(
    pm: &Market,
    ks: &Market,
    now: DateTime<Utc>,
) -> bool {
    let horizon = Duration::days(RESOLUTION_HORIZON_DAYS);
    let cutoff = now + horizon;
    let pm_far = pm
        .resolution_date
        .map(|dt| dt > cutoff)
        .unwrap_or(false);
    let ks_far = ks
        .resolution_date
        .map(|dt| dt > cutoff)
        .unwrap_or(false);
    pm_far || ks_far
}
