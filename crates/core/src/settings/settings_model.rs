//! Settings domain models.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const INSIGHTS_OVERVIEW_LAYOUT_KEY: &str = "insights_overview_layout";
pub const MARKET_DATA_SYNC_INTERVAL_HOURS_KEY: &str = "market_data_sync_interval_hours";
pub const DEFAULT_MARKET_DATA_SYNC_INTERVAL_HOURS: u32 = 6;

fn default_market_data_sync_interval_hours() -> u32 {
    DEFAULT_MARKET_DATA_SYNC_INTERVAL_HOURS
}

pub const CURRENCY_FORMATTING_REGIONS_KEY: &str = "currency_formatting_regions";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub font: String,
    pub language: String,
    pub formatting_region: String,
    /// Formatting regions for amounts in specific currencies, keyed by currency
    /// code. Other currencies follow `formatting_region`.
    #[serde(default)]
    pub currency_formatting_regions: BTreeMap<String, String>,
    pub base_currency: String,
    pub timezone: String,
    pub onboarding_completed: bool,
    pub auto_update_check_enabled: bool,
    pub menu_bar_visible: bool,
    pub sync_enabled: bool,
    #[serde(default = "default_market_data_sync_interval_hours")]
    pub market_data_sync_interval_hours: u32,
    /// Read-only restore state; clearing UI feedback must not authorize sync.
    #[serde(default)]
    pub restore_reconnect_required: bool,
    pub default_return_metric: String,
    /// Versioned dashboard preferences; the frontend validates the layout schema.
    #[serde(default)]
    pub insights_overview_layout: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "light".to_string(),
            font: "font-mono".to_string(),
            language: "en".to_string(),
            formatting_region: "system".to_string(),
            currency_formatting_regions: BTreeMap::new(),
            base_currency: "".to_string(),
            timezone: "".to_string(),
            onboarding_completed: false,
            auto_update_check_enabled: true,
            menu_bar_visible: true,
            sync_enabled: true,
            market_data_sync_interval_hours: DEFAULT_MARKET_DATA_SYNC_INTERVAL_HOURS,
            restore_reconnect_required: false,
            default_return_metric: "twr".to_string(),
            insights_overview_layout: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub theme: Option<String>,
    pub font: Option<String>,
    pub language: Option<String>,
    pub formatting_region: Option<String>,
    /// Replaces the whole map; an empty map removes every override.
    pub currency_formatting_regions: Option<BTreeMap<String, String>>,
    pub base_currency: Option<String>,
    pub timezone: Option<String>,
    pub onboarding_completed: Option<bool>,
    pub auto_update_check_enabled: Option<bool>,
    pub menu_bar_visible: Option<bool>,
    pub sync_enabled: Option<bool>,
    pub market_data_sync_interval_hours: Option<u32>,
    pub default_return_metric: Option<String>,
    pub insights_overview_layout: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub id: String,
    pub desc: bool,
}

/// Domain model for app setting key-value pair
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppSetting {
    pub setting_key: String,
    pub setting_value: String,
}
