// src/validation.rs
//! 二筛模块：对向量匹配结果进行精确验证

use regex::Regex;
use std::collections::HashMap;

/// 安全词列表（单方有日期时放行）
const SAFE_WORDS: [&str; 6] = [
    "next", "upcoming", "today", "tonight", "future", "current"
];

/// 体育比分关键词
const SPORTS_KEYWORDS: [&str; 23] = [
    "points", "goals", "runs", "o/u", "over/under", "over", "under",
    "winner", "win", "tie", "draw", "spread", "moneyline", "total",
    "vs", "versus", "score", "scored", "mvp", "championship", "points",
    "rebounds", "assists"
];

/// 体育垃圾市场关键词
const GARBAGE_KEYWORDS: [&str; 10] = [
    "o/u", "rounds", "sets", "games", "maps", "upsets",
    "quarters", "halves", "periods", "wins"
];

/// 统计数据类型（必须互斥）
const STAT_TYPES: [&str; 8] = [
    "points", "rebounds", "assists", "steals",
    "blocks", "threes", "double", "triple"
];

/// 月份名称映射
const MONTH_MAP: [(&str, u32); 24] = [
    ("jan", 1), ("january", 1),
    ("feb", 2), ("february", 2),
    ("mar", 3), ("march", 3),
    ("apr", 4), ("april", 4),
    ("may", 5), ("may", 5),
    ("jun", 6), ("june", 6),
    ("jul", 7), ("july", 7),
    ("aug", 8), ("august", 8),
    ("sep", 9), ("september", 9),
    ("oct", 10), ("october", 10),
    ("nov", 11), ("november", 11),
    ("dec", 12), ("december", 12),
];

/// 匹配结果（带方向信息）
#[derive(Debug, Clone)]
pub struct MatchInfo {
    pub pm_title: String,
    pub kalshi_title: String,
    pub similarity: f64,
    pub category: String,
    pub pm_side: String,      // "YES" 或 "NO"
    pub kalshi_side: String,  // "YES" 或 "NO"
    pub needs_inversion: bool, // 是否需要颠倒 Y/N 含义
}

/// 提取的日期信息
#[derive(Debug, Clone, PartialEq)]
pub struct DateInfo {
    pub month: u32,
    pub day: u32,
    pub has_year: bool,
    pub year: Option<u32>,
}

/// 提取的数值信息
#[derive(Debug, Clone)]
pub struct NumberInfo {
    pub value: f64,
    pub context: String,
    pub is_year: bool,
}

/// 留存样本信息
#[derive(Debug, Clone)]
pub struct RetainedSample {
    pub pm_title: String,
    pub kalshi_title: String,
    pub similarity: f64,
    pub category: String,
    pub pm_side: String,
    pub kalshi_side: String,
    pub needs_inversion: bool,
}

/// ==================== 工具函数 ====================
pub fn extract_number(text: &str) -> Option<f64> {
    let re = Regex::new(r"(\d+\.?\d*)").ok()?;
    re.captures(text)?.get(1)?.as_str().parse::<f64>().ok()
}

pub fn extract_first_team(title: &str) -> String {
    if title.contains(" vs ") {
        title.split(" vs ").next().unwrap_or("").trim().to_string()
    } else if title.contains(" vs. ") {
        title.split(" vs. ").next().unwrap_or("").trim().to_string()
    } else {
        String::new()
    }
}

fn extract_teams(title: &str) -> Option<(String, String)> {
    if title.contains(" vs ") {
        let mut parts = title.splitn(2, " vs ");
        let team1 = parts.next()?.trim().to_string();
        let team2 = parts.next()?.trim().to_string();
        if !team1.is_empty() && !team2.is_empty() {
            return Some((team1, team2));
        }
    } else if title.contains(" vs. ") {
        let mut parts = title.splitn(2, " vs. ");
        let team1 = parts.next()?.trim().to_string();
        let team2 = parts.next()?.trim().to_string();
        if !team1.is_empty() && !team2.is_empty() {
            return Some((team1, team2));
        }
    }
    None
}

pub fn extract_winner(title: &str) -> String {
    if title.contains(" - ") {
        title.split(" - ").last().unwrap_or("").trim().to_string()
    } else {
        String::new()
    }
}

fn normalize_entity_name(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut last_space = false;

    for ch in text.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            normalized.push(ch);
            last_space = false;
        } else if ch.is_whitespace() || ch == '-' || ch == '_' || ch == '\'' || ch == '’' {
            if !last_space {
                normalized.push(' ');
                last_space = true;
            }
        }
    }

    normalized.trim().to_string()
}

fn names_match(a: &str, b: &str) -> bool {
    let na = normalize_entity_name(a);
    let nb = normalize_entity_name(b);

    if na.is_empty() || nb.is_empty() {
        return false;
    }

    if na == nb {
        return true;
    }

    // 允许一个是另一个的完整子串（长度限制防止误匹配）
    if na.len() >= 4 && nb.contains(&na) {
        return true;
    }
    if nb.len() >= 4 && na.contains(&nb) {
        return true;
    }

    false
}

/// ==================== 天气/温度市场验证器 ====================
/// 温度市场必须地区一致：maximum（无地区）不能与 特定地区（如 Hong Kong）匹配
pub struct WeatherValidator;

impl WeatherValidator {
    /// 是否像温度/天气市场
    pub fn is_temperature_market(title: &str) -> bool {
        let lower = title.to_lowercase();
        (lower.contains("temperature") || lower.contains("°") || lower.contains("°c") || lower.contains("°f"))
            && (lower.contains("highest") || lower.contains("maximum") || lower.contains("minimum")
                || lower.contains("high") || lower.contains("low"))
    }

    /// 提取标题中的地区（如 "in Hong Kong" -> "hong kong"）
    pub fn extract_region(title: &str) -> Option<String> {
        // 匹配 "in X" 或 "at X"，X 为地点名（字母、空格、连字符）
        let re = Regex::new(r"(?i)(?:in|at)\s+([A-Za-z][A-Za-z\s\-']{2,}?)(?:\s+(?:be|on|,|or|\?)|$)").ok()?;
        if let Some(caps) = re.captures(title) {
            let region = caps.get(1)?.as_str().trim().to_lowercase();
            if region.len() >= 2 && !region.chars().all(|c| c == ' ' || c == '-') {
                return Some(region);
            }
        }
        // 匹配 "X temperature" 中 X 为地名（句首）
        let re2 = Regex::new(r"^(?:Will\s+(?:the\s+)?(?:highest|maximum|minimum|high|low)\s+temperature\s+in\s+)([A-Za-z][A-Za-z\s\-']+?)\s+be").ok()?;
        if let Some(caps) = re2.captures(title) {
            let region = caps.get(1)?.as_str().trim().to_lowercase();
            if region.len() >= 2 {
                return Some(region);
            }
        }
        None
    }

    /// 地区一致才通过：一方有地区一方无，或两地不同则过滤
    pub fn regions_match(pm_title: &str, kalshi_title: &str) -> bool {
        if !Self::is_temperature_market(pm_title) || !Self::is_temperature_market(kalshi_title) {
            return true; // 非温度市场，不在此处过滤
        }
        let pm_region = Self::extract_region(pm_title).map(|s| normalize_entity_name(&s));
        let ks_region = Self::extract_region(kalshi_title).map(|s| normalize_entity_name(&s));
        match (&pm_region, &ks_region) {
            (Some(p), Some(k)) => names_match(p.as_str(), k.as_str()),
            (Some(_), None) | (None, Some(_)) => false, // 一方有地区一方无，不能匹配
            (None, None) => true,                        // 都无地区（generic）则放行
        }
    }
}

/// ==================== 电竞局数验证器 ====================
/// 电竞类比赛（LoL、CS 等）需匹配 Game N / Map N / Match N，局数一致才能过二筛
pub struct EsportsGameValidator;

impl EsportsGameValidator {
    /// 提取标题中的局数：Game 4, Map 2, Match 3 等
    pub fn extract_game_number(title: &str) -> Option<u32> {
        let re = Regex::new(r"(?i)(?:game|map|match)\s*(\d+)").ok()?;
        re.captures(title)?.get(1)?.as_str().parse::<u32>().ok()
    }

    /// 是否看起来像电竞对局（有 vs 且包含 game/map/match 局数）
    pub fn is_esports_style_match(title: &str) -> bool {
        let has_vs = title.contains(" vs ") || title.contains(" vs. ");
        let has_game_keyword = Regex::new(r"(?i)(?:game|map|match)\s*\d+")
            .map(|re| re.is_match(title))
            .unwrap_or(false);
        has_vs && has_game_keyword
    }

    /// 当两边都是电竞对局时，局数必须一致才通过
    pub fn game_numbers_match(pm_title: &str, kalshi_title: &str) -> bool {
        if !Self::is_esports_style_match(pm_title) || !Self::is_esports_style_match(kalshi_title) {
            return true; // 非电竞对局，不在此处过滤
        }
        let pm_num = Self::extract_game_number(pm_title);
        let ks_num = Self::extract_game_number(kalshi_title);
        match (pm_num, ks_num) {
            (Some(p), Some(k)) => p == k,
            (Some(_), None) | (None, Some(_)) => false, // 一方有局数一方无，无法确认
            (None, None) => true,                         // 都无局数则放行
        }
    }

    /// 单局胜者（Game 3 Winner / Will X win map 3）与总局数（over 4.5 maps）不能匹配
    pub fn is_single_game_winner(title: &str) -> bool {
        let has_vs = title.contains(" vs ") || title.contains(" vs. ");
        let has_winner = Regex::new(r"(?i)(?:game|map|match)\s*\d+\s*winner")
            .map(|re| re.is_match(title))
            .unwrap_or(false);
        let has_win_map = Regex::new(r"(?i)win\s+(?:map|game|match)\s+\d+|(?:map|game|match)\s+\d+.*win")
            .map(|re| re.is_match(title))
            .unwrap_or(false);
        has_vs && (has_winner || has_win_map)
    }

    /// 总局数市场（over X maps, under X maps, X.5 maps be played）
    pub fn is_total_maps_market(title: &str) -> bool {
        let lower = title.to_lowercase();
        lower.contains("maps")
            && (lower.contains("over") || lower.contains("under"))
            && Regex::new(r"\d+\.?\d*").map(|re| re.is_match(title)).unwrap_or(false)
    }

    /// 单局胜者与总局数市场不能一起匹配
    pub fn single_vs_total_match(pm_title: &str, kalshi_title: &str) -> bool {
        let pm_single = Self::is_single_game_winner(pm_title);
        let pm_total = Self::is_total_maps_market(pm_title);
        let ks_single = Self::is_single_game_winner(kalshi_title);
        let ks_total = Self::is_total_maps_market(kalshi_title);
        // 若一方是单局胜者、另一方是总局数 -> 不匹配
        if (pm_single && ks_total) || (pm_total && ks_single) {
            return false;
        }
        true
    }

    /// 系列/BO赛果（BO5、First Stand、Group）与单局胜者不能匹配
    pub fn is_series_or_bo_market(title: &str) -> bool {
        let lower = title.to_lowercase();
        let has_vs = title.contains(" vs ") || title.contains(" vs. ");
        let has_bo = Regex::new(r"(?i)\bbo\d+\b").map(|re| re.is_match(title)).unwrap_or(false)
            || lower.contains("(bo5)") || lower.contains("(bo3)");
        let has_series = lower.contains("first stand") || lower.contains("group ")
            || lower.contains("group a") || lower.contains("group b");
        has_vs && (has_bo || has_series)
    }

    /// 单局胜者与 BO5/系列赛果 不能匹配
    pub fn single_vs_series_match(pm_title: &str, kalshi_title: &str) -> bool {
        let pm_single = Self::is_single_game_winner(pm_title);
        let pm_series = Self::is_series_or_bo_market(pm_title);
        let ks_single = Self::is_single_game_winner(kalshi_title);
        let ks_series = Self::is_series_or_bo_market(kalshi_title);
        if (pm_single && ks_series) || (pm_series && ks_single) {
            return false;
        }
        true
    }
}

/// ==================== 体育单场 vs 决赛验证器 ====================
/// 某场比赛（X at Y）与某决赛（X win Finals）不能匹配，除非确定决赛就是这两队
pub struct SportsSingleVsFinalsValidator;

impl SportsSingleVsFinalsValidator {
    /// 是否像单场比赛（A at B 格式，通常为常规赛单场）
    pub fn is_single_game_format(title: &str) -> bool {
        let lower = title.to_lowercase();
        (lower.contains(" at ") && (lower.contains("winner") || lower.contains(" - ")))
            || Regex::new(r"(?i)^[A-Za-z0-9\s]+at\s+[A-Za-z0-9\s]+")
                .map(|re| re.is_match(title))
                .unwrap_or(false)
    }

    /// 是否像决赛/系列赛（Finals, Championship, Conference Finals）
    pub fn is_finals_format(title: &str) -> bool {
        let lower = title.to_lowercase();
        lower.contains("finals") || lower.contains("championship") || lower.contains("conference finals")
    }

    /// 单场比赛与决赛不能匹配（除非能确认决赛就是这两队，此处保守过滤）
    pub fn single_vs_finals_match(pm_title: &str, kalshi_title: &str) -> bool {
        let pm_single = Self::is_single_game_format(pm_title);
        let pm_finals = Self::is_finals_format(pm_title);
        let ks_single = Self::is_single_game_format(kalshi_title);
        let ks_finals = Self::is_finals_format(kalshi_title);
        if (pm_single && ks_finals) || (pm_finals && ks_single) {
            return false;
        }
        true
    }
}

/// ==================== 垃圾市场检测 ====================
pub struct GarbageMarketDetector;

impl GarbageMarketDetector {
    pub fn is_garbage_sports_market(title: &str) -> bool {
        let lower = title.to_lowercase();
        
        // 硬规则: O/U X.X Rounds 这种直接扔
        if lower.contains("o/u") && lower.contains("rounds") {
            let upper_count = title.chars().filter(|c| c.is_uppercase()).count();
            let has_specific = lower.contains(" vs ") || 
                               lower.contains(" at ") ||
                               lower.contains(" - ") ||
                               upper_count > 2;
            
            if !has_specific {
                return true;
            }
        }
        
        // 检查垃圾关键词
        let has_garbage = GARBAGE_KEYWORDS.iter().any(|&kw| lower.contains(kw));
        if has_garbage {
            let upper_count = title.chars().filter(|c| c.is_uppercase()).count();
            let has_specific = lower.contains(" vs ") || 
                               lower.contains(" at ") ||
                               lower.contains(" - ") ||
                               upper_count > 1;
            
            if !has_specific {
                let numbers: Vec<f64> = NumberComparator::extract_numbers(title)
                    .into_iter()
                    .map(|n| n.value)
                    .collect();
                
                if !numbers.is_empty() {
                    return true;
                }
            }
        }
        
        false
    }
}

/// ==================== 胜负市场验证器 ====================
pub struct WinnerMarketValidator;

impl WinnerMarketValidator {
    pub fn validate(pm_title: &str, kalshi_title: &str) -> Option<(String, String, bool)> {
        // 检查是否都是胜负市场
        let pm_is_winner = pm_title.contains(" vs ") || pm_title.contains(" vs. ");
        let ks_is_winner = kalshi_title.contains("Winner") || kalshi_title.contains(" - ");
        
        if !pm_is_winner || !ks_is_winner {
            return None;
        }
        
        let (pm_team1, pm_team2) = extract_teams(pm_title)?;
        let ks_winner = extract_winner(kalshi_title);
        
        if ks_winner.is_empty() {
            return None;
        }
        
        // 判断是否匹配
        if names_match(&pm_team1, &ks_winner) {
            // 直接匹配：PM 买 Yes（前者胜） = Kalshi 买 Yes（前者胜）
            Some(("YES".to_string(), "YES".to_string(), false))
        } else if names_match(&pm_team2, &ks_winner) {
            // 颠倒匹配：PM 买 Yes（前者胜） = Kalshi 买 No（后者胜）
            Some(("YES".to_string(), "NO".to_string(), true))
        } else {
            None
        }
    }
}

/// ==================== 技术统计市场验证器 ====================
/// 体育球员技术统计（得分/助攻/篮板/三分）必须类型一致，且 O/U 5.5 Over 仅与 6+ 等价
pub struct StatMarketValidator;

/// 统计类型归一化（points/rebounds/assists/threes 互斥）
fn normalize_stat_type(s: &str) -> Option<&'static str> {
    let lower = s.to_lowercase();
    if lower.contains("points") && !lower.contains("rebounds") && !lower.contains("assists") {
        return Some("points");
    }
    if lower.contains("rebounds") {
        return Some("rebounds");
    }
    if lower.contains("assists") {
        return Some("assists");
    }
    if lower.contains("three") || lower.contains("threes") {
        return Some("threes");
    }
    None
}

impl StatMarketValidator {
    /// PM 格式: "Player: Rebounds O/U 4.5" 或 "Player: Assists O/U 5.5" 或 "Points Under 20.5"
    fn extract_pm_stat(title: &str) -> Option<(&'static str, f64, bool)> {
        let lower = title.to_lowercase();
        let has_ou = title.contains("O/U") || lower.contains(" over ") || lower.contains(" under ");
        if !has_ou {
            return None;
        }
        let stat = normalize_stat_type(title)?;
        let num = extract_number(title)?;
        let is_over = !lower.contains("under");
        Some((stat, num, is_over))
    }

    /// Kalshi 格式: "Player: 4+ assists" 或 "Player: 6+ rebounds - Player: 6+"
    fn extract_ks_stat(title: &str) -> Option<(&'static str, f64, bool)> {
        let re_plus = Regex::new(r"(?i)(\d+\.?\d*)\s*\+\s*([a-z]+)").ok()?;
        if let Some(caps) = re_plus.captures(title) {
            let num: f64 = caps.get(1)?.as_str().parse().ok()?;
            let word = caps.get(2)?.as_str();
            let stat = normalize_stat_type(word).or_else(|| normalize_stat_type(title))?;
            return Some((stat, num, true));
        }
        let re_minus = Regex::new(r"(?i)(\d+\.?\d*)\s*-\s*([a-z]+)").ok()?;
        if let Some(caps) = re_minus.captures(title) {
            let num: f64 = caps.get(1)?.as_str().parse().ok()?;
            let word = caps.get(2)?.as_str();
            let stat = normalize_stat_type(word).or_else(|| normalize_stat_type(title))?;
            return Some((stat, num, false));
        }
        None
    }

    /// 双方均为技术统计市场
    pub fn is_stat_market_pair(pm_title: &str, kalshi_title: &str) -> bool {
        Self::extract_pm_stat(pm_title).is_some() && Self::extract_ks_stat(kalshi_title).is_some()
    }

    /// 验证：统计类型一致且阈值严格匹配（O/U 5.5 Over ↔ 6+，与 5+ 不等价）
    pub fn validate(pm_title: &str, kalshi_title: &str) -> Option<(String, String, bool)> {
        let (pm_stat, pm_num, pm_is_over) = Self::extract_pm_stat(pm_title)?;
        let (ks_stat, ks_num, ks_is_plus) = Self::extract_ks_stat(kalshi_title)?;

        // 统计类型必须一致
        if pm_stat != ks_stat {
            return None;
        }

        // 阈值严格匹配：O/U 5.5 Over 对应 6+，不对应 5+
        let pm_threshold = if pm_is_over { pm_num.ceil() as i32 } else { pm_num.floor() as i32 };

        if ks_is_plus {
            if !pm_is_over {
                return None;
            }
            let ks_threshold = ks_num.ceil() as i32;
            if pm_threshold == ks_threshold {
                return Some(("YES".to_string(), "YES".to_string(), false));
            }
        } else {
            // ks_is_minus (Kalshi N- 表示 Under)
            if pm_is_over {
                return None;
            }
            let ks_floor = ks_num.floor() as i32;
            if pm_threshold == ks_floor {
                return Some(("YES".to_string(), "NO".to_string(), true));
            }
        }
        None
    }
}

/// ==================== 得分市场验证器 ====================
pub struct ScoreMarketValidator;

impl ScoreMarketValidator {
    /// 仅处理 Points 得分市场；Rebounds/Assists/Threes 等由 StatMarketValidator 处理
    pub fn validate(pm_title: &str, kalshi_title: &str) -> Option<(String, String, bool)> {
        // 若双方均为技术统计市场，交给 StatMarketValidator（避免 rebounds 与 assists 误配）
        if StatMarketValidator::is_stat_market_pair(pm_title, kalshi_title) {
            return None;
        }
        // 检查是否都是得分市场（Points）
        let pm_is_score = pm_title.contains("O/U") || pm_title.contains("Points");
        let ks_is_score = kalshi_title.contains('+') || kalshi_title.contains('-') ||
                          kalshi_title.contains("points");

        if !pm_is_score || !ks_is_score {
            return None;
        }
        
        let pm_num = match extract_number(pm_title) {
            Some(n) => n,
            None => return None,
        };
        
        let ks_num = match extract_number(kalshi_title) {
            Some(n) => n,
            None => return None,
        };
        
        // 判断方向
        let pm_is_over = !pm_title.to_lowercase().contains("under");
        
        let ks_is_plus = Regex::new(r"\b\d+(\.\d+)?\s*\+").ok()?.is_match(kalshi_title);
        let ks_is_minus = Regex::new(r"\b\d+(\.\d+)?\s*-").ok()?.is_match(kalshi_title);
        
        if ks_is_plus {
            // Kalshi + 表示 Over：
            // PM 允许向上取整，但必须严格等价（不再允许 ±1 档位）
            if !pm_is_over {
                return None; // 方向不一致
            }
            let pm_threshold = pm_num.ceil() as i32;
            let ks_threshold = ks_num.ceil() as i32;
            
            if pm_threshold == ks_threshold {
                return Some(("YES".to_string(), "YES".to_string(), false));
            }
        } else if ks_is_minus {
            // Kalshi - 表示 Under：
            // PM 允许向下取整，严格等价；并且套利方向需要 Y/N 颠倒
            if pm_is_over {
                return None; // 方向不一致
            }
            let pm_threshold = pm_num.floor() as i32;
            let ks_threshold = ks_num.floor() as i32;
            
            if pm_threshold == ks_threshold {
                // 方向相反，需要颠倒 Y/N
                return Some(("YES".to_string(), "NO".to_string(), true));
            }
        } else {
            // 默认按 Over 处理
            let pm_threshold = if pm_is_over { pm_num.ceil() as i32 } else { pm_num.floor() as i32 };
            let ks_threshold = if pm_is_over { ks_num.ceil() as i32 } else { ks_num.floor() as i32 };
            
            if pm_threshold == ks_threshold {
                return Some(("YES".to_string(), "YES".to_string(), false));
            }
        }
        
        None
    }
}

/// ==================== 日期验证器 ====================
pub struct DateValidator;

impl DateValidator {
    pub fn new() -> Self {
        Self
    }
    
    pub fn extract_date(text: &str) -> Option<DateInfo> {
        let text_lower = text.to_lowercase();
        
        let re = Regex::new(r"(?i)(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec|january|february|march|april|may|june|july|august|september|october|november|december)\s+(\d{1,2})(?:,?\s*(\d{4}))?").ok()?;
        
        if let Some(caps) = re.captures(text) {
            let month_name = caps.get(1)?.as_str();
            let day = caps.get(2)?.as_str().parse::<u32>().ok()?;
            
            let month = MONTH_MAP.iter()
                .find(|(name, _)| month_name.to_lowercase().contains(name))
                .map(|(_, m)| *m)?;
            
            let year = caps.get(3).and_then(|y| y.as_str().parse::<u32>().ok());
            
            return Some(DateInfo {
                month,
                day,
                has_year: year.is_some(),
                year,
            });
        }
        
        let year_re = Regex::new(r"\b(20\d{2})\b").ok()?;
        if let Some(caps) = year_re.captures(text) {
            let year = caps.get(1)?.as_str().parse::<u32>().ok()?;
            return Some(DateInfo {
                month: 0,
                day: 0,
                has_year: true,
                year: Some(year),
            });
        }
        
        None
    }
    
    pub fn has_safe_word(text: &str) -> bool {
        let text_lower = text.to_lowercase();
        SAFE_WORDS.iter().any(|&w| text_lower.contains(w))
    }
    
    pub fn dates_match(d1: &DateInfo, d2: &DateInfo) -> bool {
        if d1.month > 0 && d1.day > 0 && d2.month > 0 && d2.day > 0 {
            return d1.month == d2.month && d1.day == d2.day;
        }
        false
    }
    
    pub fn validate(&self, pm_title: &str, kalshi_title: &str) -> bool {
        let pm_date = Self::extract_date(pm_title);
        let kalshi_date = Self::extract_date(kalshi_title);
        
        match (pm_date, kalshi_date) {
            (Some(pm), Some(ks)) => Self::dates_match(&pm, &ks),
            (Some(_), None) => Self::has_safe_word(pm_title),
            (None, Some(_)) => Self::has_safe_word(kalshi_title),
            (None, None) => true,
        }
    }
}

/// ==================== 体育比分识别器 ====================
pub struct SportsIdentifier;

impl SportsIdentifier {
    pub fn new() -> Self {
        Self
    }
    
    pub fn is_sports_market(title: &str) -> bool {
        let title_lower = title.to_lowercase();
        SPORTS_KEYWORDS.iter().any(|&kw| title_lower.contains(kw))
    }
}

/// ==================== 数值比较器 ====================
pub struct NumberComparator;

impl NumberComparator {
    pub fn new() -> Self {
        Self
    }
    
    pub fn extract_numbers(text: &str) -> Vec<NumberInfo> {
        let mut numbers = Vec::new();
        let re = Regex::new(r"(\d+\.?\d*)").unwrap();
        
        for cap in re.captures_iter(text) {
            if let Ok(value) = cap[1].parse::<f64>() {
                let is_year = value >= 2000.0 && value < 2100.0;
                let context = text.to_string();
                
                numbers.push(NumberInfo {
                    value,
                    context,
                    is_year,
                });
            }
        }
        
        numbers
    }
    
    pub fn compare_numbers(nums1: &[NumberInfo], nums2: &[NumberInfo]) -> bool {
        if nums1.is_empty() != nums2.is_empty() {
            return false;
        }
        
        if nums1.is_empty() && nums2.is_empty() {
            return true;
        }
        
        for n1 in nums1 {
            for n2 in nums2 {
                if n1.is_year != n2.is_year {
                    continue;
                }
                if (n1.value - n2.value).abs() <= 0.5 {
                    return true;
                }
            }
        }
        
        false
    }
}

/// ==================== 主验证管道 ====================
pub struct ValidationPipeline {
    date_validator: DateValidator,
    pub filtered_count: usize,
    pub filtered_samples: Vec<(String, String, String)>,
    pub retained_samples: HashMap<String, Vec<RetainedSample>>,
}

impl ValidationPipeline {
    pub fn new() -> Self {
        Self {
            date_validator: DateValidator::new(),
            filtered_count: 0,
            filtered_samples: Vec::new(),
            retained_samples: HashMap::new(),
        }
    }
    
    pub fn validate(&mut self, pm_title: &str, kalshi_title: &str, similarity: f64, category: &str) -> Option<MatchInfo> {
        // 0. 垃圾市场检测
        if GarbageMarketDetector::is_garbage_sports_market(pm_title) ||
           GarbageMarketDetector::is_garbage_sports_market(kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "垃圾市场");
            return None;
        }
        
        // 1. 日期验证
        if !self.date_validator.validate(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "日期不匹配");
            return None;
        }

        // 1.1 温度市场：地区必须一致，maximum（无地区）不能与 特定地区 匹配
        if !WeatherValidator::regions_match(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "天气地区不匹配");
            return None;
        }

        // 1.2 电竞对局：局数必须一致；单局胜者与总局数市场不能匹配
        if !EsportsGameValidator::game_numbers_match(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "电竞局数不匹配");
            return None;
        }
        if !EsportsGameValidator::single_vs_total_match(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "电竞单局与总局数不能匹配");
            return None;
        }
        if !EsportsGameValidator::single_vs_series_match(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "电竞单局与BO5/系列赛不能匹配");
            return None;
        }

        // 1.3 体育：单场比赛与决赛不能匹配
        if !SportsSingleVsFinalsValidator::single_vs_finals_match(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "体育单场与决赛不能匹配");
            return None;
        }
        
        // 2. 尝试胜负市场匹配
        if let Some((pm_side, kalshi_side, needs_inversion)) = WinnerMarketValidator::validate(pm_title, kalshi_title) {
            let match_info = MatchInfo {
                pm_title: pm_title.to_string(),
                kalshi_title: kalshi_title.to_string(),
                similarity,
                category: category.to_string(),
                pm_side,
                kalshi_side,
                needs_inversion,
            };
            self.record_retained(&match_info);
            return Some(match_info);
        }
        
        // 3. 尝试得分市场匹配
        if let Some((pm_side, kalshi_side, needs_inversion)) = ScoreMarketValidator::validate(pm_title, kalshi_title) {
            let match_info = MatchInfo {
                pm_title: pm_title.to_string(),
                kalshi_title: kalshi_title.to_string(),
                similarity,
                category: category.to_string(),
                pm_side,
                kalshi_side,
                needs_inversion,
            };
            self.record_retained(&match_info);
            return Some(match_info);
        }

        // 3.5 技术统计市场：双方均为 Stat 时，必须类型一致且阈值严格匹配（O/U 5.5 Over ↔ 6+）
        if StatMarketValidator::is_stat_market_pair(pm_title, kalshi_title) {
            if let Some((pm_side, kalshi_side, needs_inversion)) = StatMarketValidator::validate(pm_title, kalshi_title) {
                let match_info = MatchInfo {
                    pm_title: pm_title.to_string(),
                    kalshi_title: kalshi_title.to_string(),
                    similarity,
                    category: category.to_string(),
                    pm_side,
                    kalshi_side,
                    needs_inversion,
                };
                self.record_retained(&match_info);
                return Some(match_info);
            } else {
                self.record_filter(pm_title, kalshi_title, "技术统计类型或阈值不匹配");
                return None;
            }
        }

        // 4. 默认数值比较
        let pm_numbers = NumberComparator::extract_numbers(pm_title);
        let kalshi_numbers = NumberComparator::extract_numbers(kalshi_title);
        
        if !NumberComparator::compare_numbers(&pm_numbers, &kalshi_numbers) {
            self.record_filter(pm_title, kalshi_title, "数值不匹配");
            return None;
        }
        
        let match_info = MatchInfo {
            pm_title: pm_title.to_string(),
            kalshi_title: kalshi_title.to_string(),
            similarity,
            category: category.to_string(),
            pm_side: "YES".to_string(),
            kalshi_side: "YES".to_string(),
            needs_inversion: false,
        };
        self.record_retained(&match_info);
        Some(match_info)
    }
    
    fn record_filter(&mut self, pm: &str, ks: &str, reason: &str) {
        self.filtered_count += 1;
        if self.filtered_count <= 3 {
            self.filtered_samples.push((pm.to_string(), ks.to_string(), reason.to_string()));
            println!("\n         🔍 二筛过滤 #{} [{}]:", self.filtered_count, reason);
            println!("            PM: {}", pm);
            println!("            Kalshi: {}", ks);
        }
    }
    
    fn record_retained(&mut self, info: &MatchInfo) {
        let sample = RetainedSample {
            pm_title: info.pm_title.clone(),
            kalshi_title: info.kalshi_title.clone(),
            similarity: info.similarity,
            category: info.category.clone(),
            pm_side: info.pm_side.clone(),
            kalshi_side: info.kalshi_side.clone(),
            needs_inversion: info.needs_inversion,
        };
        
        self.retained_samples
            .entry(info.category.clone())
            .or_insert_with(Vec::new)
            .push(sample);
    }
    
    pub fn reset_filtered_count(&mut self) {
        self.filtered_count = 0;
        self.filtered_samples.clear();
        self.retained_samples.clear();
    }
    
    pub fn print_retained_samples(&self) {
        println!("\n📊 二筛后各类别最高分样本 (每个类别最多3个):");
        
        let mut categories: Vec<_> = self.retained_samples.keys().collect();
        categories.sort();
        
        for category in categories.iter().take(5) {
            if let Some(samples) = self.retained_samples.get(*category) {
                let mut sorted = samples.clone();
                sorted.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
                
                println!("\n  类别 [{}]: {} 个留存", category, samples.len());
                for (i, sample) in sorted.iter().take(3).enumerate() {
                    let inversion_note = if sample.needs_inversion { " [Y/N颠倒]" } else { "" };
                    println!("    {}. 相似度: {:.3}{}", i+1, sample.similarity, inversion_note);
                    println!("       PM {}: {}", sample.pm_side, sample.pm_title);
                    println!("       Kalshi {}: {}", sample.kalshi_side, sample.kalshi_title);
                }
                if samples.len() > 3 {
                    println!("       ... 还有 {} 个", samples.len() - 3);
                }
            }
        }
        if categories.len() > 5 {
            println!("   ... 以及其他 {} 个类别", categories.len() - 5);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_winner_market() {
        let result = WinnerMarketValidator::validate(
            "Lakers vs Celtics",
            "Lakers vs Celtics Winner? - Lakers"
        ).unwrap();
        assert_eq!(result.0, "YES");
        assert_eq!(result.1, "YES");
        assert!(!result.2);
        
        let result = WinnerMarketValidator::validate(
            "Lakers vs Celtics",
            "Lakers vs Celtics Winner? - Celtics"
        ).unwrap();
        assert_eq!(result.0, "YES");
        assert_eq!(result.1, "NO");
        assert!(result.2);
    }
    
    #[test]
    fn test_weather_region() {
        // 香港 vs 无地区（maximum temperature）-> 地区不匹配
        assert!(!WeatherValidator::regions_match(
            "Will the highest temperature in Hong Kong be 18°C or below on March 19?",
            "Will the maximum temperature be <81° on Mar 19, 2026? - 80° or below"
        ));
        // 同地区
        assert!(WeatherValidator::regions_match(
            "Will the highest temperature in Hong Kong be 18°C or below on March 19?",
            "Will the maximum temperature in Hong Kong be <20° on Mar 19, 2026?"
        ));
        // 都无地区
        assert!(WeatherValidator::regions_match(
            "Will the maximum temperature be 20°C or below?",
            "Will the maximum temperature be <81° on Mar 19? - 80° or below"
        ));
    }

    #[test]
    fn test_esports_single_vs_series() {
        // BO5/First Stand vs 单局胜者 -> 不能匹配
        assert!(!EsportsGameValidator::single_vs_series_match(
            "LoL: G2 Esports vs BNK FEARX (BO5) - First Stand Group A",
            "Will G2 Esports win map 3 in the BNK FEARX vs. G2 Esports match? - G2 Esports"
        ));
    }

    #[test]
    fn test_sports_single_vs_finals() {
        // 决赛 vs 单场（at 格式）-> 不能匹配
        assert!(!SportsSingleVsFinalsValidator::single_vs_finals_match(
            "Will the Portland Trail Blazers win the NBA Western Conference Finals?",
            "Portland at Minnesota Winner? - Portland"
        ));
    }

    #[test]
    fn test_esports_single_vs_total() {
        // 单局胜者 vs 总局数 -> 不能匹配
        assert!(!EsportsGameValidator::single_vs_total_match(
            "LoL: G2 Esports vs BNK FEARX - Game 3 Winner",
            "Will over 4.5 maps be played in the BNK FEARX vs. G2 Esports League of Legends match? - Over 4.5 maps"
        ));
        // 单局 vs 单局 -> 可以
        assert!(EsportsGameValidator::single_vs_total_match(
            "LoL: G2 vs BNK - Game 3 Winner",
            "Will BNK win map 3 in the BNK vs. G2 match? - BNK"
        ));
    }

    #[test]
    fn test_esports_game_number() {
        // Game 4 vs Map 2 -> 局数不匹配，应过滤
        assert!(!EsportsGameValidator::game_numbers_match(
            "LoL: G2 Esports vs BNK FEARX - Game 4 Winner",
            "Will BNK FEARX win map 2 in the BNK FEARX vs. G2 Esports match? - BNK FEARX"
        ));
        // Game 4 vs Map 4 -> 局数匹配
        assert!(EsportsGameValidator::game_numbers_match(
            "LoL: G2 Esports vs BNK FEARX - Game 4 Winner",
            "Will BNK FEARX win map 4 in the BNK FEARX vs. G2 Esports match? - BNK FEARX"
        ));
        // 非电竞对局（无 game/map）-> 放行
        assert!(EsportsGameValidator::game_numbers_match(
            "Lakers vs Celtics",
            "Lakers vs Celtics Winner? - Lakers"
        ));
    }
    
    #[test]
    fn test_score_market() {
        // Points 等技术统计现由 StatMarketValidator 统一处理
        let result = StatMarketValidator::validate(
            "Points O/U 19.5",
            "20+ points"
        ).unwrap();
        assert_eq!(result.0, "YES");
        assert_eq!(result.1, "YES");
        assert!(!result.2);

        let result = StatMarketValidator::validate(
            "Points O/U 23.5",
            "25+ points"
        );
        assert!(result.is_none());

        let result = StatMarketValidator::validate(
            "Points Under 20.5",
            "20- points"
        ).unwrap();
        assert_eq!(result.0, "YES");
        assert_eq!(result.1, "NO");
        assert!(result.2);
    }

    #[test]
    fn test_stat_market_type_mismatch() {
        // Rebounds vs Assists -> 必须过滤
        assert!(StatMarketValidator::validate(
            "Ace Bailey: Rebounds O/U 4.5",
            "Ace Bailey: 4+ assists - Ace Bailey: 4+"
        ).is_none());
        assert!(StatMarketValidator::validate(
            "Paolo Banchero: Assists O/U 5.5",
            "Paolo Banchero: 5+ threes - Paolo Banchero: 5+"
        ).is_none());
    }

    #[test]
    fn test_stat_market_threshold_strict() {
        // O/U 5.5 Over 只与 6+ 匹配，不与 5+ 匹配
        assert!(StatMarketValidator::validate(
            "Tobias Harris: Rebounds O/U 5.5",
            "Tobias Harris: 5+ assists - Tobias Harris: 5+"
        ).is_none()); // 类型不同
        assert!(StatMarketValidator::validate(
            "Tobias Harris: Rebounds O/U 5.5",
            "Tobias Harris: 5+ rebounds - Tobias Harris: 5+"
        ).is_none()); // 类型相同但 5+ != 6+
        assert!(StatMarketValidator::validate(
            "Tobias Harris: Rebounds O/U 5.5",
            "Tobias Harris: 6+ rebounds - Tobias Harris: 6+"
        ).is_some()); // 正确：O/U 5.5 Over ↔ 6+
    }
}