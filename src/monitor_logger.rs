use chrono::{Utc, DateTime};
use std::fs;
use std::path::Path;
use std::io::Write;
use anyhow::Result;

const LOGS_DIR: &str = "logs";

pub struct MonitorLogger {
    logs_dir: String,
}

impl MonitorLogger {
    pub fn new(logs_dir: String) -> Result<Self> {
        fs::create_dir_all(&logs_dir)?;
        Ok(Self { logs_dir })
    }

    fn time_bucket_15m(&self, d: &DateTime<Utc>) -> String {
        let y = d.format("%Y");
        let month = d.format("%m");
        let day = d.format("%d");
        let h = d.format("%H");
        let min = (d.format("%M").to_string().parse::<i32>().unwrap_or(0) / 15) * 15;
        let min_str = format!("{:02}", min);
        format!("{}-{}-{}_{}-{}", y, month, day, h, min_str)
    }

    fn ensure_logs_dir(&self) -> Result<()> {
        Ok(fs::create_dir_all(&self.logs_dir)?)
    }

    fn append_monitor_log(&self, line: &str, at: &DateTime<Utc>) -> Result<()> {
        self.ensure_logs_dir()?;
        let bucket = self.time_bucket_15m(at);
        let filename = format!("monitor_{}.log", bucket);
        let filepath = Path::new(&self.logs_dir).join(&filename);
        
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&filepath)?;
        
        writeln!(f, "{}", line)?;
        Ok(())
    }

    pub fn log_opportunity(&self, opportunity: &crate::arbitrage_detector::ArbitrageOpportunity) -> Result<()> {
        let at = Utc::now();
        let line = if opportunity.contracts > 0.0 {
            format!(
                "[{}] 策略: {}, 本金: ${:.2}, 净利: ${:.2}, ROI: {:.1}%",
                at.to_rfc3339(),
                opportunity.strategy,
                opportunity.capital_used,
                opportunity.net_profit_100,
                opportunity.roi_100_percent
            )
        } else {
            format!(
                "[{}] 策略: {}, 成本: {:.3}, 净利: {:.3}, ROI: {:.1}%",
                at.to_rfc3339(),
                opportunity.strategy,
                opportunity.total_cost,
                opportunity.net_profit,
                opportunity.roi_percent
            )
        };
        self.append_monitor_log(&line, &at)
    }

    pub fn log_message(&self, message: &str) -> Result<()> {
        let at = Utc::now();
        let line = format!("[{}] {}", at.to_rfc3339(), message);
        self.append_monitor_log(&line, &at)
    }
}