//! Data models representing OKX API payloads consumed by the adapter.

use nautilus_core::Params;
use nautilus_model::types::Quantity;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use super::enums::OKXOptionType;
use crate::common::{
    enums::{OKXContractType, OKXInstrumentStatus, OKXInstrumentType},
    parse::deserialize_optional_string_to_u64,
};

/// Represents an instrument on the OKX exchange.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXInstrument {
    /// Product type (SPOT, MARGIN, SWAP, FUTURES, OPTION).
    pub inst_type: OKXInstrumentType,
    /// Instrument ID, e.g. "BTC-USD-SWAP".
    pub inst_id: Ustr,
    /// Instrument ID code (numeric). Required for WebSocket order operations.
    /// E.g., 10458 for BTC-USD-SWAP. May not be present for SPOT instruments.
    #[serde(default)]
    pub inst_id_code: Option<u64>,
    /// Underlying of the instrument, e.g. "BTC-USD". Only applicable to FUTURES/SWAP/OPTION.
    pub uly: Ustr,
    /// Instrument family, e.g. "BTC-USD". Only applicable to FUTURES/SWAP/OPTION.
    pub inst_family: Ustr,
    /// Base currency, e.g. "BTC" in BTC-USDT. Applicable to SPOT/MARGIN.
    pub base_ccy: Ustr,
    /// Quote currency, e.g. "USDT" in BTC-USDT.
    pub quote_ccy: Ustr,
    /// Settlement currency, e.g. "BTC" for BTC-USD-SWAP.
    pub settle_ccy: Ustr,
    /// Contract value. Only applicable to FUTURES/SWAP/OPTION.
    pub ct_val: String,
    /// Contract multiplier. Only applicable to FUTURES/SWAP/OPTION.
    pub ct_mult: String,
    /// Contract value currency. Only applicable to FUTURES/SWAP/OPTION.
    pub ct_val_ccy: String,
    /// Option type, "C" for call options, "P" for put options. Only applicable to OPTION.
    pub opt_type: OKXOptionType,
    /// Strike price. Only applicable to OPTION.
    pub stk: String,
    /// Listing time, Unix timestamp format in milliseconds, e.g. "1597026383085".
    #[serde(deserialize_with = "deserialize_optional_string_to_u64")]
    pub list_time: Option<u64>,
    /// Expiry time, Unix timestamp format in milliseconds, e.g. "1597026383085".
    #[serde(deserialize_with = "deserialize_optional_string_to_u64")]
    pub exp_time: Option<u64>,
    /// Leverage. Not applicable to SPOT.
    pub lever: String,
    /// Tick size, e.g. "0.1".
    pub tick_sz: String,
    /// Lot size, e.g. "1".
    pub lot_sz: String,
    /// Minimum order size.
    pub min_sz: String,
    /// Contract type. linear: "linear", inverse: "inverse". Only applicable to FUTURES/SWAP.
    pub ct_type: OKXContractType,
    /// Instrument status.
    pub state: OKXInstrumentStatus,
    /// Rule type, e.g. "DynamicPL", "CT", etc.
    pub rule_type: String,
    /// Maximum limit order size.
    #[serde(default)]
    pub max_lmt_sz: String,
    /// Maximum market order size.
    #[serde(default)]
    pub max_mkt_sz: String,
    /// Maximum limit order amount.
    #[serde(default)]
    pub max_lmt_amt: String,
    /// Maximum market order amount.
    #[serde(default)]
    pub max_mkt_amt: String,
    /// Maximum TWAP order size.
    #[serde(default)]
    pub max_twap_sz: String,
    /// Maximum iceberg order size.
    #[serde(default)]
    pub max_iceberg_sz: String,
    /// Maximum trigger order size.
    #[serde(default)]
    pub max_trigger_sz: String,
    /// Maximum stop order size.
    #[serde(default)]
    pub max_stop_sz: String,
}

const OKX_CT_VAL_KEY: &str = "okx_ct_val";
const OKX_CT_VAL_CCY_KEY: &str = "okx_ct_val_ccy";
const OKX_CT_TYPE_KEY: &str = "okx_ct_type";
const OKX_LOT_SZ_KEY: &str = "okx_lot_sz";
const BASE_EXPOSURE_MODE_KEY: &str = "base_exposure_mode";
const OKX_QUANTITY_UNIT_KEYS: [&str; 4] = [
    OKX_CT_VAL_KEY,
    OKX_CT_VAL_CCY_KEY,
    OKX_CT_TYPE_KEY,
    OKX_LOT_SZ_KEY,
];

/// Typed classification for how venue quantity converts to base exposure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseExposureMode {
    Identity,
    ExactMultiplier,
    PriceBased,
    Unsupported,
}

impl BaseExposureMode {
    /// Returns the stable flat string used in `instrument.info`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::ExactMultiplier => "exact_multiplier",
            Self::PriceBased => "price_based",
            Self::Unsupported => "unsupported",
        }
    }

    /// Parses a `BaseExposureMode` from a flat `instrument.info` string value.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a supported exposure mode.
    pub fn from_str(value: &str) -> anyhow::Result<Self> {
        match value {
            "identity" => Ok(Self::Identity),
            "exact_multiplier" => Ok(Self::ExactMultiplier),
            "price_based" => Ok(Self::PriceBased),
            "unsupported" => Ok(Self::Unsupported),
            _ => anyhow::bail!("Unknown base exposure mode '{value}'"),
        }
    }
}

/// Typed derivative contract types surfaced in flat OKX quantity-unit metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OkxDerivativeContractType {
    Linear,
    Inverse,
}

impl OkxDerivativeContractType {
    /// Returns the stable flat string used in `instrument.info`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Inverse => "inverse",
        }
    }

    /// Parses a derivative contract type from a flat `instrument.info` string value.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a supported derivative contract type.
    pub fn from_str(value: &str) -> anyhow::Result<Self> {
        match value {
            "linear" => Ok(Self::Linear),
            "inverse" => Ok(Self::Inverse),
            _ => anyhow::bail!("Unknown OKX contract type '{value}'"),
        }
    }

    /// Narrows a raw OKX contract type into the derivative-only helper surface.
    ///
    /// # Errors
    ///
    /// Returns an error if the OKX contract type is not a derivative contract type.
    pub fn from_okx(value: OKXContractType) -> anyhow::Result<Self> {
        match value {
            OKXContractType::Linear => Ok(Self::Linear),
            OKXContractType::Inverse => Ok(Self::Inverse),
            OKXContractType::None => anyhow::bail!("Unknown OKX contract type ''"),
        }
    }
}

/// Typed OKX quantity-unit metadata stored in flat `instrument.info` params.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OkxQuantityUnitInfo {
    /// Raw OKX `ctVal`.
    pub ct_val: String,
    /// Raw OKX `ctValCcy`.
    pub ct_val_ccy: String,
    /// Raw OKX `ctType`.
    pub ct_type: OkxDerivativeContractType,
    /// Raw OKX `lotSz`.
    pub lot_sz: String,
    /// Generic derived base exposure conversion mode.
    pub base_exposure_mode: BaseExposureMode,
}

impl OkxQuantityUnitInfo {
    /// Derives typed quantity-unit metadata for an OKX derivative instrument.
    #[must_use]
    pub fn from_derivative(
        definition: &OKXInstrument,
        base_currency: &str,
        quote_currency: &str,
        multiplier: Option<Quantity>,
    ) -> anyhow::Result<Self> {
        let ct_type = OkxDerivativeContractType::from_okx(definition.ct_type)?;
        let settlement_currency = definition.settle_ccy.as_str();
        let has_complete_derivative_metadata = !definition.ct_val.is_empty()
            && !definition.ct_mult.is_empty()
            && !definition.ct_val_ccy.is_empty()
            && !definition.lot_sz.is_empty();
        let base_exposure_mode = if !has_complete_derivative_metadata || multiplier.is_none() {
            BaseExposureMode::Unsupported
        } else if settlement_currency != base_currency && settlement_currency != quote_currency {
            BaseExposureMode::Unsupported
        } else {
            match ct_type {
                OkxDerivativeContractType::Linear if definition.ct_val_ccy == base_currency => {
                    if multiplier == Some(Quantity::from(1)) {
                        BaseExposureMode::Identity
                    } else {
                        BaseExposureMode::ExactMultiplier
                    }
                }
                OkxDerivativeContractType::Inverse if definition.ct_val_ccy == quote_currency => {
                    BaseExposureMode::PriceBased
                }
                _ => BaseExposureMode::Unsupported,
            }
        };

        Ok(Self {
            ct_val: definition.ct_val.clone(),
            ct_val_ccy: definition.ct_val_ccy.clone(),
            ct_type,
            lot_sz: definition.lot_sz.clone(),
            base_exposure_mode,
        })
    }

    /// Serializes this helper into the flat `instrument.info` layout.
    #[must_use]
    pub fn to_params(&self) -> Params {
        let mut info = Params::new();
        info.insert(
            OKX_CT_VAL_KEY.to_string(),
            Value::String(self.ct_val.clone()),
        );
        info.insert(
            OKX_CT_VAL_CCY_KEY.to_string(),
            Value::String(self.ct_val_ccy.clone()),
        );
        info.insert(
            OKX_CT_TYPE_KEY.to_string(),
            Value::String(self.ct_type.as_str().to_string()),
        );
        info.insert(
            OKX_LOT_SZ_KEY.to_string(),
            Value::String(self.lot_sz.clone()),
        );
        info.insert(
            BASE_EXPOSURE_MODE_KEY.to_string(),
            Value::String(self.base_exposure_mode.as_str().to_string()),
        );
        info
    }

    /// Deserializes typed OKX quantity-unit metadata from flat `instrument.info` params.
    ///
    /// # Errors
    ///
    /// Returns an error if any required field is missing or malformed.
    pub fn from_params(params: &Params) -> anyhow::Result<Self> {
        let info = Self {
            ct_val: raw_string(params, OKX_CT_VAL_KEY)?.to_string(),
            ct_val_ccy: raw_string(params, OKX_CT_VAL_CCY_KEY)?.to_string(),
            ct_type: OkxDerivativeContractType::from_str(required_nonempty_string(
                params,
                OKX_CT_TYPE_KEY,
            )?)?,
            lot_sz: raw_string(params, OKX_LOT_SZ_KEY)?.to_string(),
            base_exposure_mode: BaseExposureMode::from_str(required_nonempty_string(
                params,
                BASE_EXPOSURE_MODE_KEY,
            )?)?,
        };

        let has_complete_raw_metadata =
            !info.ct_val.is_empty() && !info.ct_val_ccy.is_empty() && !info.lot_sz.is_empty();
        if !has_complete_raw_metadata && info.base_exposure_mode != BaseExposureMode::Unsupported {
            anyhow::bail!(
                "Incomplete OKX quantity-unit metadata requires base_exposure_mode='unsupported'"
            );
        }

        Ok(info)
    }

    /// Deserializes typed OKX quantity-unit metadata from optional `instrument.info`.
    ///
    /// Returns `Ok(None)` when the metadata keys are absent.
    ///
    /// # Errors
    ///
    /// Returns an error if the metadata is present but malformed or incomplete.
    pub fn from_info(info: Option<&Params>) -> anyhow::Result<Option<Self>> {
        let Some(params) = info else {
            return Ok(None);
        };

        let has_any_okx_key = OKX_QUANTITY_UNIT_KEYS
            .into_iter()
            .any(|key| params.contains_key(key));

        if !has_any_okx_key {
            return Ok(None);
        }

        Self::from_params(params).map(Some)
    }
}

fn raw_string<'a>(params: &'a Params, key: &str) -> anyhow::Result<&'a str> {
    let value = params
        .get_str(key)
        .ok_or_else(|| anyhow::anyhow!("Missing OKX quantity-unit metadata key '{key}'"))?;

    Ok(value)
}

fn required_nonempty_string<'a>(params: &'a Params, key: &str) -> anyhow::Result<&'a str> {
    let value = raw_string(params, key)?;

    if value.is_empty() {
        anyhow::bail!("Empty OKX quantity-unit metadata key '{key}'");
    }

    Ok(value)
}
