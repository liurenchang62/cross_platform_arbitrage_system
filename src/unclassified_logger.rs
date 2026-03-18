// src/unclassified_logger.rs
//! 未分类日志模块：记录没有匹配到任何类别的市场

use chrono::{Local, Duration as ChronoDuration, TimeZone};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions, File};
use std::io::{Write, BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Result, Context};
use crate::market::Market;

/// 未分类日志记录
#[derive(Debug)]
struct UnclassifiedRecord {
    timestamp: String,
    market_id: String,
    platform: String,
    title: String,
    keywords: String,
}

/// 未分类日志器
pub struct UnclassifiedLogger {
    log_dir: PathBuf,
    today_records: HashSet<String>,
    current_date: String,
}

impl UnclassifiedLogger {
    /// 创建新的未分类日志器
    pub fn new<P: AsRef<Path>>(log_dir: P) -> Result<Self> {
        let log_dir = log_dir.as_ref().to_path_buf();
        fs::create_dir_all(&log_dir)?;
        
        let today = Local::now().format("%Y-%m-%d").to_string();
        
        Ok(Self {
            log_dir,
            today_records: HashSet::new(),
            current_date: today,
        })
    }
    
    /// 检查日期是否变化，如果变化则清空今日记录
    fn check_date_change(&mut self) {
        let today = Local::now().format("%Y-%m-%d").to_string();
        if today != self.current_date {
            self.today_records.clear();
            self.current_date = today;
        }
    }
    
    /// 记录未分类市场
    pub fn log_unclassified(&mut self, market: &Market) -> Result<()> {
        self.check_date_change();
        
        // 生成唯一标识用于去重
        let record_id = format!("{}:{}", market.platform, market.market_id);
        
        // 检查是否已在今日记录过
        if self.today_records.contains(&record_id) {
            return Ok(());
        }
        
        // 从标题提取关键词（长度>3的词，去重）
        let keywords: Vec<String> = market.title
            .to_lowercase()
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() > 3)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        
        let record = UnclassifiedRecord {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            market_id: market.market_id.clone(),
            platform: market.platform.clone(),
            title: market.title.clone(),
            keywords: keywords.join(","),
        };
        
        // 写入日志文件
        self.write_record(&record)?;
        
        // 记录已处理
        self.today_records.insert(record_id);
        
        Ok(())
    }
    
    /// 写入记录到文件
    fn write_record(&self, record: &UnclassifiedRecord) -> Result<()> {
        let date = Local::now().format("%Y-%m-%d");
        let log_file = self.log_dir.join(format!("unclassified-{}.csv", date));
        
        let file_exists = log_file.exists();
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .context(format!("打开日志文件失败: {:?}", log_file))?;
        
        let mut writer = std::io::BufWriter::new(file);
        
        // 如果是新文件，写入表头
        if !file_exists {
            writeln!(writer, "timestamp,market_id,platform,title,keywords")?;
        }
        
        // 写入记录
        writeln!(
            writer,
            "{},{},{},\"{}\",{}",
            record.timestamp,
            record.market_id,
            record.platform,
            record.title.replace('"', "\"\""),
            record.keywords
        )?;
        
        Ok(())
    }
    
    /// 批量记录未分类市场
    pub fn log_batch_unclassified(&mut self, markets: &[Market]) -> Result<usize> {
        let mut count = 0;
        for market in markets {
            if self.log_unclassified(market).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }
    
    /// 获取今日已记录数量
    pub fn today_record_count(&self) -> usize {
        self.today_records.len()
    }
    
    /// 获取今日日志文件路径
    pub fn get_today_log_path(&self) -> PathBuf {
        let date = Local::now().format("%Y-%m-%d");
        self.log_dir.join(format!("unclassified-{}.csv", date))
    }
    
    /// 分析最近N天的日志，统计高频关键词
    pub fn analyze_recent_logs(days: i64) -> Result<Vec<(String, usize)>> {
        let log_dir = Path::new("logs/unclassified");
        if !log_dir.exists() {
            return Ok(Vec::new());
        }
        
        let cutoff_date = Local::now() - ChronoDuration::days(days);
        let mut keyword_count: HashMap<String, usize> = HashMap::new();
        
        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            // 只处理 .csv 文件
            if path.extension().and_then(|e| e.to_str()) != Some("csv") {
                continue;
            }
            
            // 从文件名提取日期
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            let date_str = filename.replace("unclassified-", "").replace(".csv", "");
            
            // 解析日期并检查是否在范围内
            if let Ok(file_date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                let file_datetime = Local
                    .from_local_datetime(&file_date.and_hms_opt(0, 0, 0).unwrap())
                    .unwrap();
                if file_datetime < cutoff_date {
                    continue;
                }
            }
            
            // 读取文件内容
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            
            for line in reader.lines().skip(1) { // 跳过表头
                if let Ok(line) = line {
                    let fields: Vec<&str> = line.split(',').collect();
                    if fields.len() >= 5 {
                        let keywords_str = fields[4];
                        for keyword in keywords_str.split(',') {
                            if !keyword.is_empty() {
                                *keyword_count.entry(keyword.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        }
        
        // 排序并返回前30个
        let mut result: Vec<_> = keyword_count.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result.truncate(30);
        
        Ok(result)
    }
}

/// 便捷函数：快速记录未分类市场
pub fn log_unclassified_market(logger: &mut UnclassifiedLogger, market: &Market) {
    if let Err(e) = logger.log_unclassified(market) {
        eprintln!("⚠️ 记录未分类市场失败: {}", e);
    }
}