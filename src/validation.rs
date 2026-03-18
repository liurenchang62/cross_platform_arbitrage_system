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

/// ==================== 日期验证器 ====================
pub struct DateValidator;

impl DateValidator {
    pub fn new() -> Self {
        Self
    }
    
    /// 从标题中提取日期
    pub fn extract_date(text: &str) -> Option<DateInfo> {
        let text_lower = text.to_lowercase();
        
        // 匹配 "March 23" 或 "Mar 17, 2026"
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
        
        // 匹配 "2026"（纯年份）
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
        // 如果双方都有具体月日
        if d1.month > 0 && d1.day > 0 && d2.month > 0 && d2.day > 0 {
            return d1.month == d2.month && d1.day == d2.day;
        }
        // 其他情况认为不匹配（需要安全词放行）
        false
    }
    
    pub fn validate(&self, pm_title: &str, kalshi_title: &str) -> bool {
        let pm_date = Self::extract_date(pm_title);
        let kalshi_date = Self::extract_date(kalshi_title);
        
        match (pm_date, kalshi_date) {
            // 双方都有日期
            (Some(pm), Some(ks)) => {
                if Self::dates_match(&pm, &ks) {
                    true
                } else {
                    false
                }
            }
            
            // 单方有日期
            (Some(_), None) => Self::has_safe_word(pm_title),
            (None, Some(_)) => Self::has_safe_word(kalshi_title),
            
            // 双方都无日期
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
    
    /// 判断是否为体育比分市场
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
    
    /// 提取所有数值
    pub fn extract_numbers(text: &str) -> Vec<NumberInfo> {
        let mut numbers = Vec::new();
        let re = Regex::new(r"(\d+\.?\d*)").unwrap();
        
        for cap in re.captures_iter(text) {
            if let Ok(value) = cap[1].parse::<f64>() {
                // 判断是否为年份
                let is_year = value >= 2000.0 && value < 2100.0 && 
                              (text.contains("20") || text.contains("202"));
                
                // 获取上下文（数值前后20个字符）
                let start = cap.get(1).unwrap().start();
                let end = cap.get(1).unwrap().end();
                let context_start = start.saturating_sub(20);
                let context_end = (end + 20).min(text.len());
                let context = text[context_start..context_end].to_string();
                
                numbers.push(NumberInfo {
                    value,
                    context,
                    is_year,
                });
            }
        }
        
        numbers
    }
    
    /// 比较两个数值列表
    pub fn compare_numbers(nums1: &[NumberInfo], nums2: &[NumberInfo], is_sports: bool) -> bool {
        // 一方有数值，一方无数值
        if nums1.is_empty() != nums2.is_empty() {
            return false;
        }
        
        // 都无数值
        if nums1.is_empty() && nums2.is_empty() {
            return true;
        }
        
        // 都有数值，需要比较
        for n1 in nums1 {
            for n2 in nums2 {
                // 如果一个是年份另一个不是，不匹配
                if n1.is_year != n2.is_year {
                    continue;
                }
                
                if is_sports {
                    // 体育比分：允许 ±1 误差
                    if (n1.value.ceil() as i32 - n2.value.ceil() as i32).abs() <= 1 {
                        return true;
                    }
                } else {
                    // 非体育：数值必须相近
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
    pub filtered_samples: Vec<(String, String, String)>, // (pm, kalshi, reason)
    pub retained_samples: HashMap<String, Vec<RetainedSample>>, // 按类别存储留存样本
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
        // 1. 日期验证
        if !self.date_validator.validate(pm_title, kalshi_title) {
            self.record_filter(pm_title, kalshi_title, "日期不匹配");
            return false;
        }
        
        // 2. 判断是否为体育市场
        let is_sports = SportsIdentifier::is_sports_market(pm_title) || 
                        SportsIdentifier::is_sports_market(kalshi_title);
        
        // 3. 提取数值
        let pm_numbers = NumberComparator::extract_numbers(pm_title);
        let kalshi_numbers = NumberComparator::extract_numbers(kalshi_title);
        
        // 4. 数值比较
        if !NumberComparator::compare_numbers(&pm_numbers, &kalshi_numbers, is_sports) {
            self.record_filter(pm_title, kalshi_title, "数值不匹配");
            return false;
        }
        
        // 5. 记录留存样本
        self.record_retained(pm_title, kalshi_title, similarity, category);
        
        true
    }
    
    fn record_filter(&mut self, pm: &str, ks: &str, reason: &str) {
        self.filtered_count += 1;
        if self.filtered_count <= 5 {
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
    
    /// 输出每个类别的留存样本（最多5个）
    pub fn print_retained_samples(&self) {
        println!("\n📊 二筛留存样本 (每个类别最多5个):");
        
        let mut categories: Vec<_> = self.retained_samples.keys().collect();
        categories.sort();
        
        for category in categories {
            if let Some(samples) = self.retained_samples.get(category) {
                println!("\n  类别 [{}]: {} 个留存", category, samples.len());
                
                for (i, sample) in samples.iter().take(5).enumerate() {
                    println!("    {}. 相似度: {:.3}", i+1, sample.similarity);
                    println!("       PM: {}", sample.pm_title);
                    println!("       Kalshi: {}", sample.kalshi_title);
                }
                
                if samples.len() > 5 {
                    println!("       ... 还有 {} 个", samples.len() - 5);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_date_extraction() {
        let d = DateValidator::extract_date("on March 23").unwrap();
        assert_eq!(d.month, 3);
        assert_eq!(d.day, 23);
        assert!(!d.has_year);
        
        let d = DateValidator::extract_date("Mar 17, 2026").unwrap();
        assert_eq!(d.month, 3);
        assert_eq!(d.day, 17);
        assert_eq!(d.year, Some(2026));
    }
    
    #[test]
    fn test_sports_identification() {
        assert!(SportsIdentifier::is_sports_market("Points O/U 20.5"));
        assert!(SportsIdentifier::is_sports_market("Manchester City vs Real Madrid"));
        assert!(!SportsIdentifier::is_sports_market("Bitcoin price $50,000"));
    }
}