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

use hypersdk::hypercore::{
    BatchCancel, BatchCancelCloid, BatchModify, BatchOrder, Builder, Cancel, CancelByCloid,
    Cloid as SdkCloid, Modify, OidOrCloid, OrderGrouping, ScheduleCancel,
    api::{
        Action, ModifyAction, TwapOrderParams, UpdateIsolatedMargin, UpdateLeverage,
        UsdClassTransferAction, UserOutcomeAction,
    },
};

use crate::http::{
    error::{Error, Result},
    models::{
        HyperliquidExchangeAction, HyperliquidExchangeGrouping,
        HyperliquidExchangeModifyOrderRequest, HyperliquidExchangeModifyTarget,
        HyperliquidExchangeUserOutcomeOp,
    },
};

fn modify_target(target: HyperliquidExchangeModifyTarget) -> OidOrCloid {
    match target {
        HyperliquidExchangeModifyTarget::Oid(oid) => OidOrCloid::Left(oid),
        HyperliquidExchangeModifyTarget::Cloid(cloid) => OidOrCloid::Right(SdkCloid::from(cloid.0)),
    }
}

fn modify_order(modify: &HyperliquidExchangeModifyOrderRequest) -> Modify {
    Modify {
        oid: modify_target(modify.oid),
        order: (&modify.order).into(),
    }
}

/// Converts trading actions to the SDK's canonical signing representation.
pub(super) fn sdk_action(action: &HyperliquidExchangeAction) -> Result<Option<Action>> {
    let action = match action {
        HyperliquidExchangeAction::Order {
            orders,
            grouping,
            builder,
        } => {
            let grouping = match grouping {
                HyperliquidExchangeGrouping::Na => OrderGrouping::Na,
                HyperliquidExchangeGrouping::NormalTpsl => OrderGrouping::NormalTpsl,
                HyperliquidExchangeGrouping::PositionTpsl => OrderGrouping::PositionTpsl,
            };
            let builder = builder
                .as_ref()
                .map(|builder| {
                    Ok::<_, Error>(Builder {
                        builder_address: builder.address.parse().map_err(|e| {
                            Error::bad_request(format!("Invalid builder address: {e}"))
                        })?,
                        fee: builder.fee_tenths_bp,
                    })
                })
                .transpose()?;
            Action::Order(BatchOrder {
                orders: orders.iter().map(Into::into).collect(),
                grouping,
                builder,
            })
        }
        HyperliquidExchangeAction::Cancel { cancels, fast } => Action::Cancel(BatchCancel {
            cancels: cancels
                .iter()
                .map(|cancel| Cancel {
                    asset: cancel.asset as usize,
                    oid: cancel.oid,
                })
                .collect(),
            fast: fast.unwrap_or(false),
        }),
        HyperliquidExchangeAction::CancelByCloid { cancels, fast } => {
            Action::CancelByCloid(BatchCancelCloid {
                cancels: cancels
                    .iter()
                    .map(|cancel| CancelByCloid {
                        asset: cancel.asset,
                        cloid: SdkCloid::from(cancel.cloid.0),
                    })
                    .collect(),
                fast: fast.unwrap_or(false),
            })
        }
        HyperliquidExchangeAction::Modify { modify } => Action::Modify(ModifyAction {
            oid: modify_target(modify.oid),
            order: (&modify.order).into(),
            always_place: false,
        }),
        HyperliquidExchangeAction::BatchModify { modifies } => Action::BatchModify(BatchModify {
            modifies: modifies.iter().map(modify_order).collect(),
            always_place: false,
        }),
        HyperliquidExchangeAction::ScheduleCancel { time } => {
            Action::ScheduleCancel(ScheduleCancel { time: *time })
        }
        HyperliquidExchangeAction::UserOutcome { op } => Action::UserOutcome(match op {
            HyperliquidExchangeUserOutcomeOp::SplitOutcome(params) => {
                UserOutcomeAction::split(params.outcome, params.amount)
            }
            HyperliquidExchangeUserOutcomeOp::MergeOutcome(params) => {
                UserOutcomeAction::merge(params.outcome, params.amount)
            }
            HyperliquidExchangeUserOutcomeOp::MergeQuestion(params) => {
                UserOutcomeAction::merge_question(params.question, params.amount)
            }
            HyperliquidExchangeUserOutcomeOp::NegateOutcome(params) => {
                UserOutcomeAction::negate(params.question, params.outcome, params.amount)
            }
        }),
        HyperliquidExchangeAction::Noop => Action::Noop,
        HyperliquidExchangeAction::UpdateLeverage {
            asset,
            is_cross,
            leverage,
        } => Action::UpdateLeverage(UpdateLeverage {
            asset: *asset as usize,
            is_cross: *is_cross,
            leverage: *leverage,
        }),
        HyperliquidExchangeAction::TwapCancel { asset, twap_id } => Action::TwapCancel {
            a: *asset as usize,
            t: *twap_id,
        },
        HyperliquidExchangeAction::TwapPlace { twap } => Action::TwapOrder {
            twap: TwapOrderParams {
                a: twap.asset as usize,
                b: twap.is_buy,
                s: twap.size,
                r: twap.reduce_only,
                m: twap.duration_minutes,
                t: twap.randomize,
            },
        },
        HyperliquidExchangeAction::UpdateIsolatedMargin {
            asset,
            is_buy,
            ntli,
        } => {
            // The SDK uses an unsigned margin delta, preserve withdrawals as signed wire actions
            let Ok(ntli) = u64::try_from(*ntli) else {
                return Ok(None);
            };
            Action::UpdateIsolatedMargin(UpdateIsolatedMargin {
                asset: *asset as usize,
                is_buy: *is_buy,
                ntli,
            })
        }
        HyperliquidExchangeAction::UsdClassTransfer {
            hyperliquid_chain,
            signature_chain_id,
            amount,
            to_perp,
            nonce,
        } => Action::UsdClassTransfer(UsdClassTransferAction {
            signature_chain_id: format!("0x{signature_chain_id:x}"),
            hyperliquid_chain: *hyperliquid_chain,
            amount: amount.to_string(),
            to_perp: *to_perp,
            nonce: *nonce,
        }),
    };
    Ok(Some(action))
}
