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

//! Structured errors produced by [`PoolProfiler`](super::PoolProfiler) while replaying
//! historical pool events.
//!
//! Each variant carries enough context (pool, block, transaction/log position, event
//! kind, and the simulated vs. observed values that disagreed) for downstream consumers
//! to decide between hard-halt and skip-and-log on a per-pool basis.

use std::fmt::Display;

use crate::{defi::PoolIdentifier, identifiers::InstrumentId};

/// The kind of pool event being processed when an error was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolEventKind {
    Initialize,
    Swap,
    Mint,
    Burn,
    Collect,
    Flash,
}

impl Display for PoolEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Initialize => "Initialize",
            Self::Swap => "Swap",
            Self::Mint => "Mint",
            Self::Burn => "Burn",
            Self::Collect => "Collect",
            Self::Flash => "Flash",
        };
        f.write_str(name)
    }
}

/// Identifies the source event for a [`PoolProfilerError`].
#[derive(Debug, Clone)]
pub struct PoolEventLocation {
    pub instrument_id: InstrumentId,
    pub pool_identifier: PoolIdentifier,
    pub block: u64,
    pub transaction_index: u32,
    pub log_index: u32,
    pub event_kind: PoolEventKind,
}

impl Display for PoolEventLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pool={} ({}) block={} tx_index={} log_index={} event={}",
            self.instrument_id,
            self.pool_identifier,
            self.block,
            self.transaction_index,
            self.log_index,
            self.event_kind,
        )
    }
}

/// Low-level liquidity arithmetic error surfaced by
/// [`try_liquidity_math_add`](crate::defi::tick_map::liquidity_math::try_liquidity_math_add).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LiquidityMathError {
    #[error("Liquidity addition overflow: current={current}, delta={delta}")]
    Overflow { current: u128, delta: u128 },
    #[error("Liquidity subtraction underflow: current={current}, delta={delta}")]
    Underflow { current: u128, delta: u128 },
}

/// Structured errors emitted by the pool profiler during event replay.
///
/// Each variant carries enough context to identify the offending pool, event, and the
/// simulated vs. observed values that disagreed, so capture pipelines can choose
/// between hard-halt and skip-and-log on a per-pool basis.
#[derive(Debug, thiserror::Error)]
pub enum PoolProfilerError {
    #[error("Pool {instrument_id} ({pool_identifier}) already initialized")]
    AlreadyInitialized {
        instrument_id: InstrumentId,
        pool_identifier: PoolIdentifier,
    },

    #[error(
        "Pool {instrument_id} ({pool_identifier}) is not initialized while processing {event_kind}"
    )]
    NotInitialized {
        instrument_id: InstrumentId,
        pool_identifier: PoolIdentifier,
        event_kind: PoolEventKind,
    },

    #[error(
        "Initial tick mismatch for pool {instrument_id} ({pool_identifier}): pool.initial_tick={initial_tick}, computed_from_sqrt_price={calculated_tick}"
    )]
    InitialTickMismatch {
        instrument_id: InstrumentId,
        pool_identifier: PoolIdentifier,
        initial_tick: i32,
        calculated_tick: i32,
    },

    #[error("Liquidity overflow at {location}: current={current}, delta={delta}")]
    LiquidityOverflow {
        location: PoolEventLocation,
        current: u128,
        delta: u128,
    },

    #[error("Liquidity underflow at {location}: current={current}, delta={delta}")]
    LiquidityUnderflow {
        location: PoolEventLocation,
        current: u128,
        delta: u128,
    },

    #[error(
        "No events processed yet for pool {instrument_id} ({pool_identifier}); cannot extract snapshot"
    )]
    NoEventsProcessed {
        instrument_id: InstrumentId,
        pool_identifier: PoolIdentifier,
    },
}

impl PoolProfilerError {
    /// Returns the event location for variants that carry one.
    #[must_use]
    pub fn location(&self) -> Option<&PoolEventLocation> {
        match self {
            Self::LiquidityOverflow { location, .. }
            | Self::LiquidityUnderflow { location, .. } => Some(location),
            _ => None,
        }
    }
}

/// Maps a [`LiquidityMathError`] to a [`PoolProfilerError`] using event context.
#[must_use]
pub fn liquidity_error_with_location(
    err: LiquidityMathError,
    location: PoolEventLocation,
) -> PoolProfilerError {
    match err {
        LiquidityMathError::Overflow { current, delta } => PoolProfilerError::LiquidityOverflow {
            location,
            current,
            delta,
        },
        LiquidityMathError::Underflow { current, delta } => PoolProfilerError::LiquidityUnderflow {
            location,
            current,
            delta,
        },
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::address;
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn location() -> PoolEventLocation {
        PoolEventLocation {
            instrument_id: InstrumentId::from(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234.Arbitrum:UniswapV3",
            ),
            pool_identifier: PoolIdentifier::from_address(address!(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234"
            )),
            block: 12_345,
            transaction_index: 7,
            log_index: 42,
            event_kind: PoolEventKind::Burn,
        }
    }

    #[rstest]
    fn test_liquidity_error_with_location_maps_overflow(location: PoolEventLocation) {
        let err = liquidity_error_with_location(
            LiquidityMathError::Overflow {
                current: 10,
                delta: 20,
            },
            location.clone(),
        );

        match err {
            PoolProfilerError::LiquidityOverflow {
                location: out_loc,
                current,
                delta,
            } => {
                assert_eq!(current, 10);
                assert_eq!(delta, 20);
                assert_eq!(out_loc.instrument_id, location.instrument_id);
                assert_eq!(out_loc.pool_identifier, location.pool_identifier);
                assert_eq!(out_loc.block, location.block);
                assert_eq!(out_loc.transaction_index, location.transaction_index);
                assert_eq!(out_loc.log_index, location.log_index);
                assert_eq!(out_loc.event_kind, location.event_kind);
            }
            other => panic!("expected LiquidityOverflow, was {other:?}"),
        }
    }

    #[rstest]
    fn test_liquidity_error_with_location_maps_underflow(location: PoolEventLocation) {
        let err = liquidity_error_with_location(
            LiquidityMathError::Underflow {
                current: 5,
                delta: 9,
            },
            location,
        );

        match err {
            PoolProfilerError::LiquidityUnderflow { current, delta, .. } => {
                assert_eq!(current, 5);
                assert_eq!(delta, 9);
            }
            other => panic!("expected LiquidityUnderflow, was {other:?}"),
        }
    }

    #[rstest]
    fn test_pool_profiler_error_location_accessor(location: PoolEventLocation) {
        let overflow = PoolProfilerError::LiquidityOverflow {
            location: location.clone(),
            current: 1,
            delta: 2,
        };
        assert!(overflow.location().is_some());

        let underflow = PoolProfilerError::LiquidityUnderflow {
            location,
            current: 3,
            delta: 4,
        };
        assert!(underflow.location().is_some());

        let not_init = PoolProfilerError::NotInitialized {
            instrument_id: InstrumentId::from(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234.Arbitrum:UniswapV3",
            ),
            pool_identifier: PoolIdentifier::from_address(address!(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234"
            )),
            event_kind: PoolEventKind::Swap,
        };
        assert!(not_init.location().is_none());
    }

    #[rstest]
    #[case(PoolEventKind::Initialize, "Initialize")]
    #[case(PoolEventKind::Swap, "Swap")]
    #[case(PoolEventKind::Mint, "Mint")]
    #[case(PoolEventKind::Burn, "Burn")]
    #[case(PoolEventKind::Collect, "Collect")]
    #[case(PoolEventKind::Flash, "Flash")]
    fn test_pool_event_kind_display(#[case] kind: PoolEventKind, #[case] expected: &str) {
        assert_eq!(kind.to_string(), expected);
    }

    #[rstest]
    fn test_pool_event_location_display_contains_required_fields(location: PoolEventLocation) {
        let s = location.to_string();
        assert!(s.contains("0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234"));
        assert!(s.contains("Arbitrum:UniswapV3"));
        assert!(s.contains("block=12345"));
        assert!(s.contains("tx_index=7"));
        assert!(s.contains("log_index=42"));
        assert!(s.contains("event=Burn"));
    }

    #[rstest]
    fn test_pool_profiler_error_display_carries_full_context(location: PoolEventLocation) {
        // LiquidityUnderflow: pool, block, tx_index, log_index, event_kind.
        let underflow = PoolProfilerError::LiquidityUnderflow {
            location: location.clone(),
            current: 10,
            delta: 99,
        };
        let s = underflow.to_string();
        assert!(s.contains("0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234"));
        assert!(s.contains("block=12345"));
        assert!(s.contains("tx_index=7"));
        assert!(s.contains("log_index=42"));
        assert!(s.contains("event=Burn"));
        assert!(s.contains("current=10"));
        assert!(s.contains("delta=99"));

        let overflow = PoolProfilerError::LiquidityOverflow {
            location,
            current: 100,
            delta: 200,
        };
        let s = overflow.to_string();
        assert!(s.contains("current=100"));
        assert!(s.contains("delta=200"));

        let not_init = PoolProfilerError::NotInitialized {
            instrument_id: InstrumentId::from(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234.Arbitrum:UniswapV3",
            ),
            pool_identifier: PoolIdentifier::from_address(address!(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234"
            )),
            event_kind: PoolEventKind::Mint,
        };
        let s = not_init.to_string();
        assert!(s.contains("Arbitrum:UniswapV3"));
        assert!(s.contains("Mint"));
        assert!(s.contains("not initialized"));

        let mismatch = PoolProfilerError::InitialTickMismatch {
            instrument_id: InstrumentId::from(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234.Arbitrum:UniswapV3",
            ),
            pool_identifier: PoolIdentifier::from_address(address!(
                "0xBBf3209130dF7d19356d72Eb8a193e2D9Ec5c234"
            )),
            initial_tick: -100,
            calculated_tick: 200,
        };
        let s = mismatch.to_string();
        assert!(s.contains("initial_tick=-100"));
        assert!(s.contains("computed_from_sqrt_price=200"));
    }
}
