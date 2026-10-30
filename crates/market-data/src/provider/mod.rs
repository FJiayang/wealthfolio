//! Market data provider abstractions and implementations.
//!
//! This module contains:
//! - The `MarketDataProvider` trait that all providers implement
//! - Provider capabilities and rate limiting configuration
//! - Concrete provider implementations (Yahoo, Alpha Vantage, etc.)
//!
//! # Architecture
//!
//! The provider system is designed to be:
//! - **Provider-agnostic**: The core system doesn't know about specific providers
//! - **Extensible**: New providers can be added by implementing `MarketDataProvider`
//! - **Resilient**: Rate limiting and circuit breakers protect against provider failures
//!
//! # Provider Resolution
//!
//! Providers receive pre-resolved `ProviderInstrument` parameters. The resolution
//! from canonical `InstrumentId` to provider-specific parameters happens in the
//! resolver module, not in the providers themselves.

mod capabilities;
mod traits;

// Provider implementations
pub mod alpha_vantage;
pub mod amfi;
pub mod boerse_frankfurt;
pub mod eastmoney;
pub mod finnhub;
pub mod fixture;
pub mod futu;
pub mod ifind;
pub mod marketdata_app;
pub mod metal_price_api;
pub(crate) mod native;
pub mod openfigi;
pub mod sina;
pub mod tencent;
pub mod us_treasury_calc;
pub mod yahoo;

// Re-exports
pub use capabilities::{ProviderCapabilities, RateLimit};
pub use traits::MarketDataProvider;

/// Identifier shared with the custom scraper adapter in core.
pub const DATA_SOURCE_CUSTOM_SCRAPER: &str = "CUSTOM_SCRAPER";

pub const DATA_SOURCE_IFIND: &str = "IFIND";

pub const DATA_SOURCE_EASTMONEY: &str = "EASTMONEY";
pub const DATA_SOURCE_TENCENT: &str = "TENCENT";
pub const DATA_SOURCE_SINA: &str = "SINA";
pub fn is_native_provider(id: &str) -> bool {
    matches!(
        id,
        DATA_SOURCE_EASTMONEY | DATA_SOURCE_TENCENT | DATA_SOURCE_SINA
    )
}
