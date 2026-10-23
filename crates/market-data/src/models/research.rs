use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub title: String,
    pub url: String,
    pub published_at: Option<DateTime<Utc>>,
    pub publisher: Option<String>,
    pub summary: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FinancialStatement {
    BalanceSheet,
    IncomeStatement,
    CashFlow,
}

/// Preserve the source's accounting taxonomy and units instead of inventing equivalences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialReport {
    pub statement: FinancialStatement,
    pub period_end: NaiveDate,
    /// Native fiscal period label (annual and quarterly reports may share a date).
    pub period_label: Option<String>,
    pub published_at: Option<NaiveDate>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    pub items: BTreeMap<String, Value>,
    pub source: String,
}
