// query_params.rs
//! API 查询参数统一管理

// ==================== 通用参数 ====================
/// 请求间隔（毫秒）
pub const REQUEST_INTERVAL_MS: u64 = 200;

/// 最大重试次数
pub const MAX_RETRIES: u32 = 3;

/// 重试初始等待时间（毫秒）
pub const RETRY_INITIAL_DELAY_MS: u64 = 1000;

// ==================== Polymarket 参数 ====================
/// Polymarket 单次请求最大事件数
pub const POLYMARKET_PAGE_LIMIT: u32 = 200;

/// Polymarket 最大获取市场数
pub const POLYMARKET_MAX_MARKETS: usize = 20000;

// ==================== Kalshi 参数 ====================
/// Kalshi 每页市场数
pub const KALSHI_PAGE_LIMIT: u32 = 1000;

/// Kalshi 最大获取市场数
pub const KALSHI_MAX_MARKETS: usize = 20000;

// ==================== 向量化参数 ====================
/// 最大词汇表大小（降维用）
/// Some(500) 表示限制到500维，None 表示无上限
pub const MAX_VOCAB_SIZE: Option<usize> = None;

// ==================== 匹配参数 ====================
/// 相似度阈值
pub const SIMILARITY_THRESHOLD: f64 = 0.8;

/// 每个 (query, 类别) 在索引中保留的候选条数（精确按余弦排序后截断）。
/// 仅当提高此值时才会扩大初筛候选集；降低可能影响召回，变更前应做审计对比。
pub const SIMILARITY_TOP_K: usize = 7;

/// 全量获取周期（每 N 个追踪周期执行一次全量获取）
pub const FULL_FETCH_INTERVAL: usize = 180;

// ==================== 市场时间窗口 ====================
/// 仅保留解析日在「当前 UTC 时间 + 本天数」及以内的市场；无解析日期的市场保留。
/// 有日期且解析日晚于该截止的剔除。
pub const RESOLUTION_HORIZON_DAYS: i64 = 21;