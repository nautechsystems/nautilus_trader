// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use pyo3::prelude::*;

use crate::{
    common::{
        enums::BybitProductType,
        parse::{parse_tp_sl_order_type, parse_tpsl_mode, parse_trigger_type},
    },
    http::query::BybitNativeTpSlParams as RustNativeTpSlParams,
};

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::enums::{BybitOrderType, BybitTpSlMode, BybitTriggerType};

    #[rstest]
    fn test_native_tp_sl_params_try_from_accepts_valid_enums() {
        let params = BybitNativeTpSlParams {
            take_profit: Some("55000".to_string()),
            stop_loss: Some("47000".to_string()),
            tp_trigger_by: Some("LastPrice".to_string()),
            sl_trigger_by: Some("MarkPrice".to_string()),
            tp_order_type: Some("Limit".to_string()),
            sl_order_type: Some("Market".to_string()),
            tpsl_mode: Some("Partial".to_string()),
            ..Default::default()
        };

        let native = RustNativeTpSlParams::try_from(params).unwrap();

        assert_eq!(native.tp_trigger_by, Some(BybitTriggerType::LastPrice));
        assert_eq!(native.sl_trigger_by, Some(BybitTriggerType::MarkPrice));
        assert_eq!(native.tp_order_type, Some(BybitOrderType::Limit));
        assert_eq!(native.sl_order_type, Some(BybitOrderType::Market));
        assert_eq!(native.tpsl_mode, Some(BybitTpSlMode::Partial));
    }

    #[rstest]
    #[case("tp_trigger_by")]
    #[case("sl_trigger_by")]
    #[case("tp_order_type")]
    #[case("sl_order_type")]
    #[case("tpsl_mode")]
    fn test_native_tp_sl_params_try_from_rejects_invalid_enum(#[case] field: &str) {
        let mut params = BybitNativeTpSlParams::default();
        match field {
            "tp_trigger_by" => params.tp_trigger_by = Some("garbage".to_string()),
            "sl_trigger_by" => params.sl_trigger_by = Some("garbage".to_string()),
            "tp_order_type" => params.tp_order_type = Some("garbage".to_string()),
            "sl_order_type" => params.sl_order_type = Some("garbage".to_string()),
            "tpsl_mode" => params.tpsl_mode = Some("garbage".to_string()),
            _ => unreachable!(),
        }

        let err = RustNativeTpSlParams::try_from(params).unwrap_err();
        assert!(err.to_string().contains("garbage"));
    }

    #[rstest]
    fn test_native_tp_sl_params_try_from_rejects_unknown_tpsl_mode() {
        // `BybitTpSlMode` has a `#[serde(other)] Unknown` variant; a raw deserialize would
        // silently accept "Unknown". The validated parser must reject it.
        let params = BybitNativeTpSlParams {
            tpsl_mode: Some("Unknown".to_string()),
            ..Default::default()
        };

        let err = RustNativeTpSlParams::try_from(params).unwrap_err();
        assert!(err.to_string().contains("invalid Bybit TP/SL mode"));
    }
}

/// Parameters for fetching tickers via HTTP API.
#[pyclass(from_py_object)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bybit")]
#[derive(Clone, Debug)]
pub struct BybitTickersParams {
    #[pyo3(get, set)]
    pub category: BybitProductType,
    #[pyo3(get, set)]
    pub symbol: Option<String>,
    #[pyo3(get, set)]
    pub base_coin: Option<String>,
    #[pyo3(get, set)]
    pub exp_date: Option<String>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitTickersParams {
    /// Query parameters for `GET /v5/market/tickers`.
    ///
    /// # References
    /// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
    #[new]
    #[pyo3(signature = (category, symbol=None, base_coin=None, exp_date=None))]
    fn py_new(
        category: BybitProductType,
        symbol: Option<String>,
        base_coin: Option<String>,
        exp_date: Option<String>,
    ) -> Self {
        Self {
            category,
            symbol,
            base_coin,
            exp_date,
        }
    }
}

impl From<BybitTickersParams> for crate::http::query::BybitTickersParams {
    fn from(params: BybitTickersParams) -> Self {
        Self {
            category: params.category,
            symbol: params.symbol,
            base_coin: params.base_coin,
            exp_date: params.exp_date,
        }
    }
}

/// Native TP/SL and option-specific fields for `POST /v5/order/create` (used by the demo HTTP
/// path, since demo does not expose the mainnet WS Trade API).
///
/// Enum-typed fields are accepted as strings and parsed at the binding boundary.
#[pyclass(from_py_object)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bybit")]
#[derive(Debug, Clone, Default)]
pub struct BybitNativeTpSlParams {
    #[pyo3(get, set)]
    pub take_profit: Option<String>,
    #[pyo3(get, set)]
    pub stop_loss: Option<String>,
    #[pyo3(get, set)]
    pub tp_trigger_by: Option<String>,
    #[pyo3(get, set)]
    pub sl_trigger_by: Option<String>,
    #[pyo3(get, set)]
    pub tp_order_type: Option<String>,
    #[pyo3(get, set)]
    pub sl_order_type: Option<String>,
    #[pyo3(get, set)]
    pub tp_limit_price: Option<String>,
    #[pyo3(get, set)]
    pub sl_limit_price: Option<String>,
    #[pyo3(get, set)]
    pub tpsl_mode: Option<String>,
    #[pyo3(get, set)]
    pub close_on_trigger: Option<bool>,
    #[pyo3(get, set)]
    pub order_iv: Option<String>,
    #[pyo3(get, set)]
    pub mmp: Option<bool>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitNativeTpSlParams {
    /// Native TP/SL and option-specific fields that map onto the `POST /v5/order/create` entry.
    ///
    /// Bundled to keep the `submit_order` signature manageable, and to give the demo HTTP path
    /// access to the same fields the mainnet WS path supports via
    /// `crate.websocket.messages.BybitWsPlaceOrderParams`. All fields are optional; populated
    /// fields are written onto the entry builder as-is. `tpsl_mode` defaults to `Full` upstream when
    /// only `take_profit` / `stop_loss` are set without an explicit mode.
    ///
    /// `tp_trigger_price` / `sl_trigger_price` are intentionally absent: the create-order entry does
    /// not carry them (the mainnet WS Trade API does, via separate fields).
    #[new]
    #[pyo3(signature = (
        take_profit=None,
        stop_loss=None,
        tp_trigger_by=None,
        sl_trigger_by=None,
        tp_order_type=None,
        sl_order_type=None,
        tp_limit_price=None,
        sl_limit_price=None,
        tpsl_mode=None,
        close_on_trigger=None,
        order_iv=None,
        mmp=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        take_profit: Option<String>,
        stop_loss: Option<String>,
        tp_trigger_by: Option<String>,
        sl_trigger_by: Option<String>,
        tp_order_type: Option<String>,
        sl_order_type: Option<String>,
        tp_limit_price: Option<String>,
        sl_limit_price: Option<String>,
        tpsl_mode: Option<String>,
        close_on_trigger: Option<bool>,
        order_iv: Option<String>,
        mmp: Option<bool>,
    ) -> Self {
        Self {
            take_profit,
            stop_loss,
            tp_trigger_by,
            sl_trigger_by,
            tp_order_type,
            sl_order_type,
            tp_limit_price,
            sl_limit_price,
            tpsl_mode,
            close_on_trigger,
            order_iv,
            mmp,
        }
    }
}

impl TryFrom<BybitNativeTpSlParams> for RustNativeTpSlParams {
    type Error = anyhow::Error;

    fn try_from(params: BybitNativeTpSlParams) -> anyhow::Result<Self> {
        Ok(Self {
            take_profit: params.take_profit,
            stop_loss: params.stop_loss,
            tp_trigger_by: params
                .tp_trigger_by
                .as_deref()
                .map(parse_trigger_type)
                .transpose()?,
            sl_trigger_by: params
                .sl_trigger_by
                .as_deref()
                .map(parse_trigger_type)
                .transpose()?,
            tp_order_type: params
                .tp_order_type
                .as_deref()
                .map(parse_tp_sl_order_type)
                .transpose()?,
            sl_order_type: params
                .sl_order_type
                .as_deref()
                .map(parse_tp_sl_order_type)
                .transpose()?,
            tp_limit_price: params.tp_limit_price,
            sl_limit_price: params.sl_limit_price,
            tpsl_mode: params
                .tpsl_mode
                .as_deref()
                .map(parse_tpsl_mode)
                .transpose()?,
            close_on_trigger: params.close_on_trigger,
            order_iv: params.order_iv,
            mmp: params.mmp,
        })
    }
}
