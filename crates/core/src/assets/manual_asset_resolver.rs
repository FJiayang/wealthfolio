//! Asset resolution shared by manual activity entry and trade instructions.
use super::{
    canonicalize_market_identity, normalize_quote_ccy_code, parse_crypto_pair_symbol,
    parse_symbol_with_known_exchange, resolve_bond_aliases, resolve_quote_ccy_precedence, Asset,
    AssetKind, AssetServiceTrait, AssetSpec, InstrumentType, QuoteCcyResolutionSource, QuoteMode,
};
use crate::activities::is_garbage_symbol;
use crate::activities::ActivityError;
use crate::errors::{DatabaseError, Error};
use crate::Result;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use wealthfolio_market_data::mic_to_currency;

pub struct ManualAssetResolver {
    asset_service: Arc<dyn AssetServiceTrait>,
}
impl ManualAssetResolver {
    pub fn new(asset_service: Arc<dyn AssetServiceTrait>) -> Self {
        Self { asset_service }
    }
    pub async fn resolve(
        &self,
        mut input: ManualAssetInput,
        currency: String,
        require_asset: bool,
        custom_option_multiplier: Option<Decimal>,
    ) -> Result<Option<(String, Asset)>> {
        // Extract asset fields from nested `asset` object
        let symbol = input.symbol.as_deref().map(|s| s.to_string());
        let exchange_mic = input.exchange_mic.as_deref().map(|s| s.to_string());
        let asset_kind_input = input.kind.as_deref().map(|s| s.to_string());
        let quote_ccy_input = Self::normalize_quote_ccy(input.quote_ccy.as_deref());
        let instrument_type_input = Self::parse_instrument_type(input.instrument_type.as_deref());
        let asset_name = input.name.as_deref().map(|s| s.to_string());
        let provider_id_input = input.provider_id.clone();
        let provider_symbol_input = input.provider_symbol.clone();
        let quote_mode = input.quote_mode.as_deref().map(|s| s.to_string());
        let parsed_quote_mode =
            quote_mode
                .as_deref()
                .and_then(|mode| match mode.to_uppercase().as_str() {
                    "MANUAL" => Some(QuoteMode::Manual),
                    "MARKET" => Some(QuoteMode::Market),
                    _ => None,
                });

        let inferred = symbol.as_deref().map(|s| {
            self.infer_asset_kind(s, exchange_mic.as_deref(), asset_kind_input.as_deref())
        });
        let inferred_instrument_type = inferred.as_ref().and_then(|(_, it)| it.clone());
        let effective_instrument_type = instrument_type_input
            .clone()
            .or(inferred_instrument_type.clone());
        let effective_kind = instrument_type_input
            .as_ref()
            .map(Self::kind_from_instrument_type)
            .or_else(|| inferred.as_ref().map(|(kind, _)| kind.clone()));

        // Normalize symbol + MIC using payload/suffix only (no live lookup for user save paths).
        let is_crypto = effective_instrument_type.as_ref() == Some(&InstrumentType::Crypto);
        let is_option = effective_instrument_type.as_ref() == Some(&InstrumentType::Option);
        let is_non_security_instrument = matches!(
            effective_instrument_type.as_ref(),
            Some(InstrumentType::Crypto | InstrumentType::Fx)
        );
        // A supplied MIC decides whether a trailing `.X` is a venue or part of the
        // ticker: `ZAAA.F` on Cboe Canada is a hedged unit class, not a Frankfurt
        // listing. See `parse_symbol_with_known_exchange`.
        let (base_symbol, suffix_mic) = symbol
            .as_deref()
            .map(|sym| parse_symbol_with_known_exchange(sym, exchange_mic.as_deref()))
            .unwrap_or(("", None));
        let exchange_mic = if is_non_security_instrument {
            None
        } else {
            exchange_mic.or_else(|| suffix_mic.map(|mic| mic.to_string()))
        };
        let normalized_symbol_for_lookup = if base_symbol.is_empty() {
            None
        } else if is_crypto {
            Some(
                parse_crypto_pair_symbol(base_symbol)
                    .map(|(base, _)| base)
                    .unwrap_or_else(|| base_symbol.to_string()),
            )
        } else if is_option {
            // Normalize broker-specific option symbols (e.g. Fidelity's "-MU270115C600")
            // to standard OCC format before storing.
            Some(
                crate::utils::occ_symbol::normalize_option_symbol(base_symbol)
                    .unwrap_or_else(|| base_symbol.to_string()),
            )
        } else {
            Some(base_symbol.to_string())
        };
        let submitted_asset_id = self.resolved_submitted_asset_id(
            input.id.as_deref(),
            normalized_symbol_for_lookup.as_deref(),
            exchange_mic.as_deref(),
            effective_instrument_type.as_ref(),
            quote_ccy_input.as_deref(),
        )?;
        input.id = submitted_asset_id.clone();

        match symbol.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw_symbol) => {
                if is_garbage_symbol(raw_symbol) {
                    return Err(ActivityError::InvalidData(format!(
                        "Invalid symbol '{}'. Please search for a valid ticker.",
                        raw_symbol
                    ))
                    .into());
                }

                self.asset_service.validate_persisted_symbol_metadata(
                    normalized_symbol_for_lookup
                        .as_deref()
                        .unwrap_or(raw_symbol),
                    submitted_asset_id.as_deref(),
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    parsed_quote_mode,
                    quote_ccy_input.as_deref(),
                )?;
            }
            None if input
                .id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .is_none()
                && require_asset =>
            {
                return Err(ActivityError::InvalidData(
                    "Asset-backed activities need either asset_id or symbol".to_string(),
                )
                .into());
            }
            None => {}
        }

        // Use pair quote for crypto/FX; otherwise resolve from payload and existing data:
        // explicit input -> existing asset -> MIC fallback -> activity/account.
        let mut quote_ccy_for_asset = quote_ccy_input.clone();
        let asset_currency = if is_crypto {
            symbol
                .as_deref()
                .and_then(parse_crypto_pair_symbol)
                .map(|(_, quote)| quote)
                .or_else(|| quote_ccy_input.clone())
                .unwrap_or_else(|| currency.clone())
        } else if is_non_security_instrument {
            quote_ccy_input.clone().unwrap_or(currency.clone())
        } else {
            let existing_asset_quote_ccy = self
                .existing_asset_quote_ccy_by_id(submitted_asset_id.as_deref())
                .or_else(|| {
                    normalized_symbol_for_lookup
                        .as_deref()
                        .and_then(|resolved_symbol| {
                            self.asset_service.existing_quote_ccy_by_symbol(
                                resolved_symbol,
                                exchange_mic.as_deref(),
                                effective_instrument_type.as_ref(),
                            )
                        })
                });
            let (resolved_quote_ccy, resolution_source) = resolve_quote_ccy_precedence(
                quote_ccy_input.as_deref(),
                existing_asset_quote_ccy.as_deref(),
                None,
                exchange_mic.as_deref().and_then(mic_to_currency),
                Some(currency.as_str()),
            )
            .unwrap_or_else(|| (currency.clone(), QuoteCcyResolutionSource::TerminalFallback));
            if matches!(
                resolution_source,
                QuoteCcyResolutionSource::ExplicitInput | QuoteCcyResolutionSource::ProviderQuote
            ) {
                quote_ccy_for_asset = Some(resolved_quote_ccy.clone());
            }
            resolved_quote_ccy
        };

        // Resolve asset_id:
        // 1. If symbol is provided, search existing assets or prepare for creation
        // 2. If only asset.id is provided (UUID), use it directly
        // 3. Cash activities: no asset
        let resolved_asset_id = if let Some(ref normalized_symbol) = normalized_symbol_for_lookup {
            // Look up existing asset by instrument fields
            let existing_id = self
                .find_existing_asset_id(
                    normalized_symbol,
                    exchange_mic.as_deref(),
                    effective_instrument_type.as_ref(),
                    Some(&asset_currency),
                )
                .or_else(|| submitted_asset_id.clone());

            if let Some(id) = existing_id {
                Some(id)
            } else {
                // Create new asset with generated UUID
                let new_id = Uuid::new_v4().to_string();

                // Build structured metadata for option/bond/metal assets
                let structured_metadata = if let Some(mult) = custom_option_multiplier {
                    crate::assets::build_option_metadata(normalized_symbol, mult)
                } else {
                    crate::assets::build_asset_metadata(
                        effective_instrument_type.as_ref(),
                        normalized_symbol,
                    )
                };

                let metadata = crate::assets::AssetMetadata {
                    name: asset_name.clone(),
                    kind: effective_kind.clone(),
                    instrument_exchange_mic: exchange_mic.clone(),
                    instrument_symbol: Some(normalized_symbol.clone()),
                    instrument_type: effective_instrument_type.clone(),
                    display_code: Some(normalized_symbol.clone()),
                    requested_quote_ccy: quote_ccy_for_asset.clone(),
                    provider_config: None,
                    provider_id: provider_id_input.clone(),
                    provider_symbol: provider_symbol_input.clone(),
                    asset_metadata: structured_metadata,
                };
                self.asset_service
                    .get_or_create_minimal_asset(
                        &new_id,
                        Some(asset_currency.clone()),
                        Some(metadata),
                        quote_mode.clone(),
                    )
                    .await?;
                Some(new_id)
            }
        } else if let Some(asset_id) = submitted_asset_id.as_deref() {
            // Existing asset_id provided (UUID from frontend)
            Some(asset_id.to_string())
        } else if !require_asset {
            None // Symbol-optional types have no asset when symbol is absent
        } else {
            return Err(ActivityError::InvalidData(
                "Asset-backed activities need either asset_id or symbol".to_string(),
            )
            .into());
        };

        // Process asset if asset_id is resolved
        if let Some(ref asset_id) = resolved_asset_id {
            let canonical_symbol = normalized_symbol_for_lookup.clone();
            let metadata = crate::assets::AssetMetadata {
                name: asset_name.clone(),
                kind: effective_kind,
                instrument_exchange_mic: exchange_mic.clone(),
                instrument_symbol: canonical_symbol.clone(),
                instrument_type: effective_instrument_type.clone(),
                display_code: canonical_symbol,
                requested_quote_ccy: quote_ccy_for_asset.clone(),
                provider_config: None,
                provider_id: provider_id_input,
                provider_symbol: provider_symbol_input,
                asset_metadata: None,
            };
            let mut asset = if normalized_symbol_for_lookup.is_none() {
                self.asset_service.get_asset_by_id(asset_id)?
            } else {
                self.asset_service
                    .get_or_create_minimal_asset(
                        asset_id,
                        Some(asset_currency.clone()),
                        Some(metadata),
                        quote_mode.clone(),
                    )
                    .await?
            };

            // Update asset quote mode if specified (for existing assets that need mode change)
            if let Some(ref mode) = quote_mode {
                let requested_mode = mode.to_uppercase();
                let current_mode = asset.quote_mode.as_db_str();
                if requested_mode != current_mode {
                    asset = self
                        .asset_service
                        .update_quote_mode_silent(&asset.id, &requested_mode)
                        .await?;
                }
            }

            Ok(Some((asset_id.clone(), asset)))
        } else {
            Ok(None)
        }
    }
    pub(crate) fn parse_instrument_type(value: Option<&str>) -> Option<InstrumentType> {
        match value?.trim().to_uppercase().as_str() {
            "EQUITY" | "STOCK" | "ETF" | "MUTUALFUND" | "MUTUAL_FUND" | "INDEX" | "FUTURE"
            | "FUTURES" => Some(InstrumentType::Equity),
            "CRYPTO" | "CRYPTOCURRENCY" => Some(InstrumentType::Crypto),
            "FX" | "FOREX" | "CURRENCY" => Some(InstrumentType::Fx),
            "OPTION" => Some(InstrumentType::Option),
            "METAL" | "COMMODITY" => Some(InstrumentType::Metal),
            "BOND" | "FIXEDINCOME" | "FIXED_INCOME" | "DEBT" | "MONEYMARKET" => {
                Some(InstrumentType::Bond)
            }
            _ => None,
        }
    }
    pub(crate) fn normalize_quote_ccy(value: Option<&str>) -> Option<String> {
        let trimmed = value.map(str::trim).filter(|s| !s.is_empty())?;
        if trimmed.eq_ignore_ascii_case("GBP") {
            return Some("GBP".to_string());
        }
        if trimmed == "GBp" {
            return Some("GBp".to_string());
        }
        if trimmed.eq_ignore_ascii_case("GBX") {
            return Some("GBX".to_string());
        }
        if trimmed == "ZAc" || trimmed.eq_ignore_ascii_case("ZAC") {
            return Some("ZAc".to_string());
        }
        if !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        if !(3..=5).contains(&trimmed.len()) {
            return None;
        }
        Some(trimmed.to_uppercase())
    }
    pub(crate) fn kind_from_instrument_type(instrument_type: &InstrumentType) -> AssetKind {
        match instrument_type {
            InstrumentType::Fx => AssetKind::Fx,
            _ => AssetKind::Investment,
        }
    }
    pub(crate) fn existing_asset_quote_ccy_by_id(&self, asset_id: Option<&str>) -> Option<String> {
        let id = asset_id?.trim();
        if id.is_empty() {
            return None;
        }
        self.asset_service
            .get_asset_by_id(id)
            .ok()
            .and_then(|asset| normalize_quote_ccy_code(Some(asset.quote_ccy.as_str())))
    }
    pub(crate) fn infer_asset_kind(
        &self,
        symbol: &str,
        exchange_mic: Option<&str>,
        asset_kind_input: Option<&str>,
    ) -> (AssetKind, Option<InstrumentType>) {
        // 1. If explicit input is provided, use it
        if let Some(asset_kind_value) = asset_kind_input {
            match asset_kind_value.to_uppercase().as_str() {
                "SECURITY" | "INVESTMENT" | "EQUITY" => {
                    return (AssetKind::Investment, Some(InstrumentType::Equity))
                }
                "CRYPTO" => return (AssetKind::Investment, Some(InstrumentType::Crypto)),
                "FX_RATE" | "FX" => return (AssetKind::Fx, Some(InstrumentType::Fx)),
                "OPTION" | "OPT" => return (AssetKind::Investment, Some(InstrumentType::Option)),
                "BOND" => return (AssetKind::Investment, Some(InstrumentType::Bond)),
                "COMMODITY" | "CMDTY" | "METAL" => {
                    return (AssetKind::Investment, Some(InstrumentType::Metal))
                }
                "PROPERTY" | "PROP" => return (AssetKind::Property, None),
                "VEHICLE" | "VEH" => return (AssetKind::Vehicle, None),
                "COLLECTIBLE" | "COLL" => return (AssetKind::Collectible, None),
                "PRECIOUS_METAL" | "PREC" => return (AssetKind::PreciousMetal, None),
                "PRIVATE_EQUITY" | "PEQ" => return (AssetKind::PrivateEquity, None),
                "LIABILITY" | "LIAB" => return (AssetKind::Liability, None),
                // A custom "Other" asset bought in an account is held there: an
                // investment, not the alternative kind net worth adds on its own.
                "OTHER" | "ALT" => return (AssetKind::Investment, None),
                _ => {} // Fall through to inference
            }
        }

        // 2. Crypto pair pattern (e.g., BTC-USD, ETH-CAD) — checked before
        //    exchange_mic because brokers may attach their MIC to crypto pairs
        let upper_symbol = symbol.to_uppercase();
        if let Some((_base, quote)) = upper_symbol.rsplit_once('-') {
            let quote = quote.trim();
            let crypto_quotes = [
                "USD", "CAD", "EUR", "GBP", "JPY", "CHF", "AUD", "NZD", "HKD", "SGD", "CNY", "SEK",
                "NOK", "DKK", "PLN", "CZK", "HUF", "TRY", "MXN", "BRL", "KRW", "INR", "ZAR", "BTC",
                "ETH", "USDT", "USDC", "DAI", "BUSD", "USDP", "TUSD", "FDUSD",
            ];
            if crypto_quotes.contains(&quote) {
                return (AssetKind::Investment, Some(InstrumentType::Crypto));
            }
        }

        // 3. OCC option symbol heuristic (e.g. AAPL240119C00150000)
        // Must be checked before exchange MIC — search providers may attach an
        // exchange MIC (e.g. "OPRA") to option symbols, which would otherwise
        // cause them to be misclassified as equities.
        if crate::utils::occ_symbol::looks_like_occ_symbol(&upper_symbol) {
            return (AssetKind::Investment, Some(InstrumentType::Option));
        }

        // 4. If exchange MIC is provided, it's an equity
        if exchange_mic.is_some() {
            return (AssetKind::Investment, Some(InstrumentType::Equity));
        }

        // 5. Common crypto symbols heuristic (no MIC, bare symbol like BTC, ETH)
        let common_crypto = [
            "BTC", "ETH", "XRP", "LTC", "BCH", "ADA", "DOT", "LINK", "XLM", "DOGE", "UNI", "SOL",
            "AVAX", "MATIC", "ATOM", "ALGO", "VET", "FIL", "TRX", "ETC", "XMR", "AAVE", "MKR",
            "COMP", "SNX", "YFI", "SUSHI", "CRV",
        ];
        if common_crypto.contains(&upper_symbol.as_str()) {
            return (AssetKind::Investment, Some(InstrumentType::Crypto));
        }

        // 6. Default to equity (most common case)
        (AssetKind::Investment, Some(InstrumentType::Equity))
    }
    pub(crate) fn is_asset_not_found_error(err: &Error) -> bool {
        matches!(err, Error::Database(DatabaseError::NotFound(_)))
    }
    pub(crate) fn has_submitted_asset_identity(
        submitted_symbol: Option<&str>,
        submitted_exchange_mic: Option<&str>,
        submitted_instrument_type: Option<&InstrumentType>,
        submitted_quote_ccy: Option<&str>,
    ) -> bool {
        submitted_symbol
            .map(str::trim)
            .filter(|symbol| !symbol.is_empty())
            .is_some()
            || submitted_exchange_mic
                .map(str::trim)
                .filter(|mic| !mic.is_empty())
                .is_some()
            || submitted_instrument_type.is_some()
            || submitted_quote_ccy
                .map(str::trim)
                .filter(|ccy| !ccy.is_empty())
                .is_some()
    }
    pub(crate) fn asset_matches_submitted_identity(
        existing_asset: &crate::assets::Asset,
        submitted_symbol: Option<&str>,
        submitted_exchange_mic: Option<&str>,
        submitted_instrument_type: Option<&InstrumentType>,
        submitted_quote_ccy: Option<&str>,
    ) -> bool {
        let submitted_identity = canonicalize_market_identity(
            submitted_instrument_type.cloned(),
            submitted_symbol,
            submitted_exchange_mic,
            submitted_quote_ccy,
        );
        let existing_identity = canonicalize_market_identity(
            existing_asset.instrument_type.clone(),
            existing_asset
                .instrument_symbol
                .as_deref()
                .or(existing_asset.display_code.as_deref()),
            existing_asset.instrument_exchange_mic.as_deref(),
            Some(existing_asset.quote_ccy.as_str()),
        );

        if let Some(submitted_type) = submitted_instrument_type {
            if existing_asset.instrument_type.as_ref() != Some(submitted_type) {
                return false;
            }
        }

        if let Some(submitted_symbol) = submitted_identity.instrument_symbol.as_deref() {
            if Some(submitted_symbol) != existing_identity.instrument_symbol.as_deref() {
                return false;
            }
        }

        match submitted_instrument_type.or(existing_asset.instrument_type.as_ref()) {
            Some(InstrumentType::Crypto | InstrumentType::Fx) => {
                if let Some(submitted_quote_ccy) = submitted_identity.quote_ccy.as_deref() {
                    return Some(submitted_quote_ccy) == existing_identity.quote_ccy.as_deref();
                }
                true
            }
            Some(InstrumentType::Option) => true,
            _ => {
                if let Some(submitted_mic) = submitted_identity.instrument_exchange_mic.as_deref() {
                    return Some(submitted_mic)
                        == existing_identity.instrument_exchange_mic.as_deref();
                }
                true
            }
        }
    }
    pub(crate) fn resolved_submitted_asset_id(
        &self,
        submitted_asset_id: Option<&str>,
        submitted_symbol: Option<&str>,
        submitted_exchange_mic: Option<&str>,
        submitted_instrument_type: Option<&InstrumentType>,
        submitted_quote_ccy: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(asset_id) = submitted_asset_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return Ok(None);
        };

        let has_identity = Self::has_submitted_asset_identity(
            submitted_symbol,
            submitted_exchange_mic,
            submitted_instrument_type,
            submitted_quote_ccy,
        );

        match self.asset_service.get_asset_by_id(asset_id) {
            Ok(existing_asset) => {
                if has_identity
                    && !Self::asset_matches_submitted_identity(
                        &existing_asset,
                        submitted_symbol,
                        submitted_exchange_mic,
                        submitted_instrument_type,
                        submitted_quote_ccy,
                    )
                {
                    Ok(None)
                } else {
                    Ok(Some(asset_id.to_string()))
                }
            }
            Err(err) if has_identity && Self::is_asset_not_found_error(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }
    pub(crate) fn find_existing_asset_id(
        &self,
        symbol: &str,
        exchange_mic: Option<&str>,
        instrument_type: Option<&InstrumentType>,
        quote_ccy: Option<&str>,
    ) -> Option<String> {
        let assets = self.asset_service.get_assets().unwrap_or_default();
        let upper_symbol = symbol.to_uppercase();
        let expected_key = instrument_type.and_then(|itype| match itype {
            InstrumentType::Crypto | InstrumentType::Fx => quote_ccy.and_then(|ccy| {
                let normalized_ccy = ccy.trim().to_uppercase();
                if normalized_ccy.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{}:{}/{}",
                        itype.as_db_str(),
                        upper_symbol,
                        normalized_ccy
                    ))
                }
            }),
            _ => exchange_mic
                .filter(|mic| !mic.trim().is_empty())
                .map(|mic| {
                    format!(
                        "{}:{}@{}",
                        itype.as_db_str(),
                        upper_symbol,
                        mic.trim().to_uppercase()
                    )
                })
                .or_else(|| Some(format!("{}:{}", itype.as_db_str(), upper_symbol))),
        });

        // Fallback key for OCC option symbols that were previously misclassified
        // as EQUITY due to exchange MIC taking priority over OCC heuristic.
        // Must mirror the key format the old code would have produced (with MIC when present).
        let fallback_equity_key = if matches!(instrument_type, Some(InstrumentType::Option)) {
            exchange_mic
                .filter(|mic| !mic.trim().is_empty())
                .map(|mic| {
                    format!(
                        "{}:{}@{}",
                        InstrumentType::Equity.as_db_str(),
                        upper_symbol,
                        mic.trim().to_uppercase()
                    )
                })
                .or_else(|| {
                    Some(format!(
                        "{}:{}",
                        InstrumentType::Equity.as_db_str(),
                        upper_symbol,
                    ))
                })
        } else {
            None
        };

        if let Some(ref key) = expected_key {
            // Pass 1: exact instrument key match
            for asset in &assets {
                if asset.instrument_key.as_deref() == Some(key) {
                    return Some(asset.id.clone());
                }
            }
            // Pass 2: fallback for legacy misclassified options
            if let Some(ref fallback) = fallback_equity_key {
                for asset in &assets {
                    if asset.instrument_key.as_deref() == Some(fallback.as_str()) {
                        return Some(asset.id.clone());
                    }
                }
            }
        }

        if instrument_type == Some(&InstrumentType::Bond) {
            let specs = [AssetSpec::market_instrument(
                symbol.to_string(),
                symbol.to_string(),
                exchange_mic.map(str::to_string),
                InstrumentType::Bond,
                quote_ccy.unwrap_or("USD").to_string(),
            )];
            if let Some(id) = resolve_bond_aliases(&specs, &assets)[0].asset_id.clone() {
                return Some(id);
            }
        }

        for asset in &assets {
            if let (Some(ref a_symbol), Some(ref a_type)) =
                (&asset.instrument_symbol, &asset.instrument_type)
            {
                let type_matches = instrument_type.is_none_or(|t| t == a_type);
                let symbol_matches = a_symbol.to_uppercase() == upper_symbol;
                let mic_matches = if matches!(a_type, InstrumentType::Option) {
                    // OCC option ticker is globally unique; tolerate legacy MIC mismatch to avoid duplicates.
                    match (exchange_mic, &asset.instrument_exchange_mic) {
                        (Some(mic), Some(a_mic)) => mic.eq_ignore_ascii_case(a_mic),
                        _ => true,
                    }
                } else {
                    match (exchange_mic, &asset.instrument_exchange_mic) {
                        (Some(mic), Some(a_mic)) => mic.eq_ignore_ascii_case(a_mic),
                        (None, None) => true,
                        _ => false,
                    }
                };
                let ccy_matches = if matches!(a_type, InstrumentType::Crypto | InstrumentType::Fx) {
                    quote_ccy.is_none_or(|ccy| asset.quote_ccy.eq_ignore_ascii_case(ccy))
                } else {
                    true
                };
                if type_matches && symbol_matches && mic_matches && ccy_matches {
                    return Some(asset.id.clone());
                }
            }
        }
        None
    }
}

/// Input for asset identification when creating/updating activities.
/// Consolidates all asset-related fields into a single nested object.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManualAssetInput {
    /// Existing asset ID. When provided without natural identity, it must refer to a persisted asset.
    pub id: Option<String>,
    /// Symbol (e.g., "AAPL", "BTC") used to resolve or create an asset.
    pub symbol: Option<String>,
    /// Exchange MIC code (e.g., "XNAS", "XTSE") for securities.
    pub exchange_mic: Option<String>,
    /// Asset kind hint (e.g., "SECURITY", "CRYPTO") - if not provided, inferred.
    pub kind: Option<String>,
    /// Asset name hint for custom/manual assets.
    pub name: Option<String>,
    /// Quote mode hint: "MARKET" or "MANUAL".
    pub quote_mode: Option<String>,
    /// Optional quote currency from symbol search/provider (e.g., "GBp").
    pub quote_ccy: Option<String>,
    /// Optional instrument type from symbol search/provider (e.g., "EQUITY", "CRYPTO").
    pub instrument_type: Option<String>,
    /// Market data provider that resolved this symbol, if selected.
    pub provider_id: Option<String>,
    /// Provider-native symbol/code selected by search/import.
    pub provider_symbol: Option<String>,
}

impl ManualAssetInput {
    /// An empty object is the explicit PATCH representation for removing an
    /// activity's optional asset. Omitting the object preserves the asset.
    pub fn is_empty(&self) -> bool {
        self.id.is_none()
            && self.symbol.is_none()
            && self.exchange_mic.is_none()
            && self.kind.is_none()
            && self.name.is_none()
            && self.quote_mode.is_none()
            && self.quote_ccy.is_none()
            && self.instrument_type.is_none()
            && self.provider_id.is_none()
            && self.provider_symbol.is_none()
    }
}
