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

/// 体育垃圾市场关键词（删除后只剩数值的）
const GARBAGE_KEYWORDS: [&str; 8] = [
    "o/u", "rounds", "sets", "games", "maps", "quarters", "halves", "periods"
];

/// 统计数据类型（必须互斥）
const STAT_TYPES: [&str; 8] = [
    "points", "rebounds", "assists", "steals",
    "blocks", "threes", "double", "triple"
];

/// 胜负方向关键词
const WINNER_KEYWORDS: [&str; 3] = ["winner", "win", "victory"];
const DRAW_KEYWORDS: [&str; 3] = ["draw", "tie", "push"];

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
}

/// ==================== 垃圾市场检测 ====================
/// ==================== 垃圾市场检测 ====================
pub struct GarbageMarketDetector;

impl GarbageMarketDetector {
    /// 检测是否为垃圾体育市场（如 "O/U 2.5 Rounds"）
    pub fn is_garbage_sports_market(title: &str) -> bool {
        let lower = title.to_lowercase();
        
        // 硬规则1: O/U X.X Rounds 这种直接扔
        if lower.contains("o/u") && lower.contains("rounds") {
            // 检查是否有具体的人名/队名（大写字母 > 2 表示有人名）
            let upper_count = title.chars().filter(|c| c.is_uppercase()).count();
            
            // 检查是否有具体的比赛信息
            let has_specific = lower.contains(" vs ") || 
                               lower.contains(" at ") ||
                               lower.contains(" - ") ||
                               upper_count > 2;
            
            // 如果没有具体信息，就是垃圾
            if !has_specific {
                return true;
            }
        }
        
        // 硬规则2: 只有数值和泛泛关键词，没有具体内容的
        let garbage_keywords = ["o/u", "rounds", "sets", "games", "maps", "upsets"];
        let has_garbage = garbage_keywords.iter().any(|&kw| lower.contains(kw));
        
        if has_garbage {
            // 提取所有大写字母（人名/队名的标志）
            let upper_count = title.chars().filter(|c| c.is_uppercase()).count();
            
            // 检查是否有具体信息
            let has_specific = lower.contains(" vs ") || 
                               lower.contains(" at ") ||
                               lower.contains(" - ") ||
                               title.contains(' ') && upper_count > 1;
            
            // 如果没有具体信息，且只有数字和垃圾词，就是垃圾
            if !has_specific {
                // 提取数字
                let numbers: Vec<f64> = NumberComparator::extract_numbers(title)
                    .into_iter()
                    .map(|n| n.value)
                    .collect();
                
                // 如果至少有1个数字，且没有具体信息，就是垃圾
                if !numbers.is_empty() {
                    return true;
                }
            }
        }
        
        false
    }
}




/// ==================== 胜负方向识别 ====================
pub struct WinnerDirection;

impl WinnerDirection {
    /// 判断是否为胜平负市场
    fn is_winner_market(title: &str) -> bool {
        let lower = title.to_lowercase();
        WINNER_KEYWORDS.iter().any(|&kw| lower.contains(kw)) ||
        lower.contains("vs") || lower.contains("versus")
    }
    
    /// 判断是否为平局市场
    fn is_draw_market(title: &str) -> bool {
        let lower = title.to_lowercase();
        DRAW_KEYWORDS.iter().any(|&kw| lower.contains(kw))
    }
    
    /// 检查胜负方向是否互斥
    pub fn check_direction(pm_title: &str, kalshi_title: &str) -> bool {
        let pm_is_winner = Self::is_winner_market(pm_title);
        let pm_is_draw = Self::is_draw_market(pm_title);
        let ks_is_winner = Self::is_winner_market(kalshi_title);
        let ks_is_draw = Self::is_draw_market(kalshi_title);
        
        if (pm_is_winner && ks_is_draw) || (pm_is_draw && ks_is_winner) {
            return false;
        }
        
        true
    }
}

/// ==================== 统计数据互斥 ====================
pub struct StatTypeChecker;

impl StatTypeChecker {
    /// 提取统计数据类型
    fn extract_stat_type(title: &str) -> Option<&'static str> {
        let lower = title.to_lowercase();
        for &stat in STAT_TYPES.iter() {
            if lower.contains(stat) {
                return Some(stat);
            }
        }
        None
    }
    
    /// 检查统计数据类型是否兼容
    pub fn check_compatibility(pm_title: &str, kalshi_title: &str) -> bool {
        let pm_stat = Self::extract_stat_type(pm_title);
        let ks_stat = Self::extract_stat_type(kalshi_title);
        
        match (pm_stat, ks_stat) {
            (Some(p), Some(k)) => p == k,
            (Some(_), None) => false,
            (None, Some(_)) => false,
            (None, None) => true,
        }
    }
}

/// ==================== 日期验证器 ====================
pub struct DateValidator;

impl DateValidator {
    pub fn new() -> Self {
        Self
    }
    
    /// 从标题中提取日期
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
    
    /// 检查是否包含安全词
    pub fn has_safe_word(text: &str) -> bool {
        let text_lower = text.to_lowercase();
        SAFE_WORDS.iter().any(|&w| text_lower.contains(w))
    }
    
    /// 比较两个日期（只比较月日）
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
    
    /// 提取所有数值（修复字符边界问题）
    pub fn extract_numbers(text: &str) -> Vec<NumberInfo> {
        let mut numbers = Vec::new();
        let re = Regex::new(r"(\d+\.?\d*)").unwrap();
        
        for cap in re.captures_iter(text) {
            if let Ok(value) = cap[1].parse::<f64>() {
                let is_year = value >= 2000.0 && value < 2100.0;
                
                // 直接用整个字符串作为上下文，避免字符边界问题
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
    
    /// 比较两个数值列表（使用原始值，允许 ±0.5 误差）
    pub fn compare_numbers(nums1: &[NumberInfo], nums2: &[NumberInfo], is_sports: bool) -> bool {
        // 一方有数值，一方无数值
        if nums1.is_empty() != nums2.is_empty() {
            return false;
        }
        
        // 都无数值
        if nums1.is_empty() && nums2.is_empty() {
            return true;
        }
        
        for n1 in nums1 {
            for n2 in nums2 {
                if n1.is_year != n2.is_year {
                    continue;
                }
                
                if is_sports {
                    // 体育比分：比较原始值，允许 ±0.5 误差
                    if (n1.value - n2.value).abs() <= 0.5 {
                        return true;
                    }
                } else {
                    // 非体育：数值必须相近（允许 ±1.0 误差）
                    if (n1.value - n2.value).abs() < 1.0 {
                        return true;
                    }
                }
            }
        }
        
        false
    }
}

/// ==================== 主验证管道 ====================
pub struct ValidationPipeline {
    date_validator: DateValidator,
    number_comparator: NumberComparator,
    pub filtered_count: usize,
    pub filtered_samples: Vec<(String, String, String)>,
    pub retained_samples: HashMap<String, Vec<RetainedSample>>,
}

impl ValidationPipeline {
    pub fn new() -> Self {
        Self {
            date_validator: DateValidator::new(),
            number_comparator: NumberComparator::new(),
            filtered_count: 0,
            filtered_samples: Vec::new(),
            retained_samples: HashMap::new(),
        }
    }
    
    pub fn validate(&mut self, pm_title: &str, kalshi_title: &str, similarity: f64, category: &str) -> bool {
        // 0. 垃圾市场检测
        if GarbageMarketDetector::is_garbage_sports_market(pm_title) ||
           GarbageMarketDetector::is_garbage_sports_market(kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "垃圾市场");
            return false;
        }
        
        // 1. 胜负方向互斥检查
        if !WinnerDirection::check_direction(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "胜负方向冲突");
            return false;
        }
        
        // 2. 统计数据类型互斥检查
        if !StatTypeChecker::check_compatibility(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "统计数据类型不匹配");
            return false;
        }
        
        // 3. 日期验证
        if !self.date_validator.validate(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "日期不匹配");
            return false;
        }
        
        // 4. 判断是否为体育市场
        let is_sports = SportsIdentifier::is_sports_market(pm_title) || 
                        SportsIdentifier::is_sports_market(kalshi_title);
        
        // 5. 数值比较
        let pm_numbers = NumberComparator::extract_numbers(pm_title);
        let kalshi_numbers = NumberComparator::extract_numbers(kalshi_title);
        
        if !NumberComparator::compare_numbers(&pm_numbers, &kalshi_numbers, is_sports) {
            self.record_filter(pm_title, kalshi_title, "数值不匹配");
            return false;
        }
        
        self.record_retained(pm_title, kalshi_title, similarity, category);
        true
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
    
    fn record_retained(&mut self, pm: &str, ks: &str, similarity: f64, category: &str) {
        let sample = RetainedSample {
            pm_title: pm.to_string(),
            kalshi_title: ks.to_string(),
            similarity,
            category: category.to_string(),
        };
        
        self.retained_samples
            .entry(category.to_string())
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
                    println!("    {}. 相似度: {:.3}", i+1, sample.similarity);
                    println!("       PM: {}", sample.pm_title);
                    println!("       Kalshi: {}", sample.kalshi_title);
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
    fn test_garbage_detector() {
        assert!(GarbageMarketDetector::is_garbage_sports_market("O/U 2.5 Rounds"));
        assert!(!GarbageMarketDetector::is_garbage_sports_market("Nikola Jokić: Points O/U 20.5"));
        assert!(!GarbageMarketDetector::is_garbage_sports_market("Lakers vs Celtics"));
    }
    
    #[test]
    fn test_number_comparison() {
        let nums1 = vec![NumberInfo { value: 5.5, context: "".to_string(), is_year: false }];
        let nums2 = vec![NumberInfo { value: 6.0, context: "".to_string(), is_year: false }];
        let nums3 = vec![NumberInfo { value: 7.0, context: "".to_string(), is_year: false }];
        
        assert!(NumberComparator::compare_numbers(&nums1, &nums2, true));  // 5.5 vs 6.0 ✅
        assert!(!NumberComparator::compare_numbers(&nums1, &nums3, true)); // 5.5 vs 7.0 ❌
    }
}