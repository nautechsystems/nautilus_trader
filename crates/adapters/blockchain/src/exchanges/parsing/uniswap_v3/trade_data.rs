// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use alloy::primitives::{Signed, U160, U256};
use nautilus_model::{
    defi::Token,
    enums::OrderSide,
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};

use crate::{events::swap::SwapEvent, math::convert_i256_to_f64};

/// <https://blog.uniswap.org/uniswap-v3-math-primer>
fn calculate_price_from_sqrt_price(
    sqrt_price_x96: U160,
    token0_decimals: u8,
    token1_decimals: u8,
) -> f64 {
    // Convert sqrt_price_x96 to U256 for better precision
    let sqrt_price_u256 = U256::from(sqrt_price_x96);

    // Calculate price = (sqrt_price_x96 / 2^96)^2
    // Which is equivalent to: sqrt_price_x96^2 / 2^192
    let price_x192 = sqrt_price_u256 * sqrt_price_u256;

    // Convert to f64 maintaining precision
    // Price = price_x192 / 2^192
    let price_str = price_x192.to_string();
    let price_x192_f64: f64 = price_str.parse().unwrap_or(f64::INFINITY);

    // 2^192 as f64
    let two_pow_192: f64 = (1u128 << 96) as f64 * (1u128 << 96) as f64;
    let price_raw = price_x192_f64 / two_pow_192;

    // Adjust for decimal differences
    // The raw price is in terms of raw token amounts (token1_raw / token0_raw)
    // To get human readable price (token1 per token0), we need to adjust:
    // price_human = price_raw * (10^token0_decimals / 10^token1_decimals)
    let decimal_adjustment = 10f64.powi(i32::from(token0_decimals) - i32::from(token1_decimals));

    price_raw * decimal_adjustment
}

/// Converts a Uniswap V3 swap event to trade data.
///
/// # Errors
///
/// Returns an error if price or quantity calculations fail or if values are invalid.
pub fn convert_to_trade_data(
    token0: &Token,
    token1: &Token,
    swap_event: &SwapEvent,
) -> anyhow::Result<(OrderSide, Quantity, Price)> {
    let price_f64 = calculate_price_from_sqrt_price(
        swap_event.sqrt_price_x96,
        token0.decimals,
        token1.decimals,
    );

    // Validate price is finite and positive
    if !price_f64.is_finite() || price_f64 <= 0.0 {
        anyhow::bail!(
            "Invalid price calculated from sqrt_price_x96: {}, result: {}",
            swap_event.sqrt_price_x96,
            price_f64
        );
    }

    // Additional validation for extremely small or large prices - removed arbitrary bounds
    // The Price::from constructor will handle actual validation using PRICE_MIN/PRICE_MAX

    let price = Price::from(format!(
        "{:.precision$}",
        price_f64,
        precision = FIXED_PRECISION as usize
    ));

    let quantity_f64 = convert_i256_to_f64(swap_event.amount1, token1.decimals)?.abs();

    // Validate quantity is finite and non-negative
    if !quantity_f64.is_finite() || quantity_f64 < 0.0 {
        anyhow::bail!(
            "Invalid quantity calculated from amount1: {}, result: {}",
            swap_event.amount1,
            quantity_f64
        );
    }

    let quantity = Quantity::from(format!(
        "{:.precision$}",
        quantity_f64,
        precision = FIXED_PRECISION as usize
    ));

    let zero = Signed::<256, 4>::ZERO;
    let side = if swap_event.amount1 > zero {
        OrderSide::Buy // User receives token1 (buys token1)
    } else {
        OrderSide::Sell // User gives token1 (sells token1)
    };
    Ok((side, quantity, price))
}
