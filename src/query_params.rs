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

/// Polymarket 最大获取市场数（5万封顶）
pub const POLYMARKET_MAX_MARKETS: usize = 20000;  // 从 50000 降到 30000

// ==================== Kalshi 参数 ====================
/// Kalshi 每页市场数
pub const KALSHI_PAGE_LIMIT: u32 = 1000;

/// Kalshi 最大获取市场数（5万封顶）
pub const KALSHI_MAX_MARKETS: usize = 20000;  // 从 50000 降到 30000

// ==================== 向量化参数 ====================
/// 最大词汇表大小（降维用）
pub const MAX_VOCAB_SIZE: usize = 5000;

// ==================== 匹配参数 ====================
/// 相似度阈值
pub const SIMILARITY_THRESHOLD: f64 = 0.7;

/// 全量获取周期（每 N 个追踪周期执行一次全量获取）
pub const FULL_FETCH_INTERVAL: usize = 720;