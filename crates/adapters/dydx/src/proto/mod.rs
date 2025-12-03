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

//! Protocol Buffer definitions for dYdX v4.
//!
//! Re-exports proto definitions from the `dydx_proto` crate which includes both
//! Cosmos SDK and dYdX protocol-specific messages.

// Re-export commonly used proto types
// Re-export entire modules for comprehensive access
pub use dydx_proto::{
    ToAny, cosmos_sdk_proto,
    cosmos_sdk_proto::cosmos::{
        auth::v1beta1::{
            BaseAccount, QueryAccountRequest, query_client::QueryClient as AuthClient,
        },
        bank::v1beta1::{
            MsgSend, QueryAllBalancesRequest, query_client::QueryClient as BankClient,
        },
        base::{
            tendermint::v1beta1::{
                Block, GetLatestBlockRequest, GetNodeInfoRequest, GetNodeInfoResponse,
                service_client::ServiceClient as BaseClient,
            },
            v1beta1::Coin,
        },
        tx::v1beta1::{
            BroadcastMode, BroadcastTxRequest, GetTxRequest, SimulateRequest,
            service_client::ServiceClient as TxClient,
        },
    },
    dydxprotocol,
    dydxprotocol::{
        accountplus::TxExtension,
        clob::{
            ClobPair, MsgBatchCancel, MsgCancelOrder, MsgPlaceOrder, Order, OrderBatch, OrderId,
            QueryAllClobPairRequest,
            order::{
                self as order_proto, ConditionType, Side as OrderSide,
                TimeInForce as OrderTimeInForce,
            },
            query_client::QueryClient as ClobClient,
        },
        perpetuals::{
            Perpetual, QueryAllPerpetualsRequest, query_client::QueryClient as PerpetualsClient,
        },
        sending::{MsgCreateTransfer, MsgDepositToSubaccount, MsgWithdrawFromSubaccount, Transfer},
        subaccounts::{
            QueryGetSubaccountRequest, Subaccount as SubaccountInfo, SubaccountId,
            query_client::QueryClient as SubaccountsClient,
        },
    },
};
