//! SQLite storage implementation for market data.

mod model;
mod quote_sync_state_repository;
mod repository;
mod request_counts;

pub use model::{
    MarketDataProviderSettingDB, QuoteDB, QuoteSyncStateDB, QuoteSyncStateUpdateDB,
    UpdateMarketDataProviderSettingDB,
};
pub use quote_sync_state_repository::QuoteSyncStateRepository;

// Re-export trait from core for convenience
pub use repository::MarketDataRepository;
pub use request_counts::ProviderRequestSqliteRepository;
pub use wealthfolio_core::quotes::SyncStateStore;
