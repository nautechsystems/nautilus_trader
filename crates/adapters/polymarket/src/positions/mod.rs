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

//! Deposit Wallet split, merge, and redeem operations for Polymarket positions.

pub mod amounts;
pub mod calldata;
pub mod convert;
pub mod effects;
pub mod metadata;
pub mod record;

mod wallet;

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use alloy_primitives::{Address, U256};
use nautilus_common::messages::execution::PositionOperationStatus;
use nautilus_core::{
    UUID4,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::position_operation::{PositionOperationLeg, PositionOperationType};
use nautilus_network::websocket::proxy::ProxyUrl;
use parking_lot::Mutex;
use rust_decimal::Decimal;

use self::{
    amounts::pusd_to_base_units,
    calldata::{
        PositionCall, encode_approval, encode_convert_positions, encode_merge_positions,
        encode_merge_positions_v2, encode_redeem_position_v2, encode_redeem_positions,
        encode_split_position, encode_split_position_v2, parse_condition_id_v2,
    },
    convert::ConvertIndexSet,
    effects::PositionOperationEffects,
    metadata::{NegRiskMarket, validate_market_id},
    record::{
        InMemoryPositionOperationStore, ManagedOperationIntent, PositionOperationAppliedLeg,
        PositionOperationRecord, PositionOperationRecordStore, PositionOperationRecorder,
        PositionOperationStoreRecorder, PositionUnsignedCall,
    },
};
use crate::{
    common::{
        consts::MARKET_VERSION_V2,
        credential::{EvmPrivateKey, RelayerApiKey},
        parse::{validate_data_market_version, validate_outcome_ids, validate_outcomes},
    },
    http::{
        clob::PolymarketClobPublicClient,
        error::{Error, Result},
        relayer::{
            PolymarketRelayerHttpClient, RelayerTransaction, RelayerTransactionState,
            RelayerWalletSubmit,
        },
    },
    signing::eip712::{
        DepositWalletCall, OrderSigner, approval_plan_v2, parse_address, parse_bytes32,
    },
};

const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 60;
const DEFAULT_DEADLINE_SECS: u64 = 1_800;
const DEFAULT_WAIT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;

static WALLET_SUBMISSIONS: LazyLock<Mutex<HashMap<Address, Arc<WalletSubmissionState>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A managed position operation request prepared by the execution client.
///
/// The request supplies the on-chain parameters; the [`ManagedOperationIntent`] supplies the
/// initiator, ownership, instruments, and quantity recorded before submission.
#[derive(Clone, Debug, PartialEq)]
pub enum ManagedPositionOperationRequest {
    /// Splits `amount` pUSD into both outcome tokens of `condition_id`.
    Split {
        /// The CLOB condition identifier.
        condition_id: String,
        /// The pUSD amount.
        amount: Decimal,
    },
    /// Merges `amount` complete sets of `condition_id` back into pUSD.
    Merge {
        /// The CLOB condition identifier.
        condition_id: String,
        /// The per-outcome share quantity.
        amount: Decimal,
    },
    /// Redeems both CTF outcome balances of `condition_id`.
    RedeemCtf {
        /// The CLOB condition identifier.
        condition_id: String,
    },
    /// Redeems an explicit V2 outcome quantity.
    RedeemV2 {
        /// The CLOB condition identifier.
        condition_id: String,
        /// The V2 outcome index, `0` or `1`.
        outcome_index: u8,
        /// The share quantity.
        amount: Decimal,
    },
    /// Converts selected NO positions of a negative-risk event.
    Convert {
        /// The on-chain negative-risk market identifier.
        market_id: String,
        /// The selected question indices as a bitmask.
        index_set: U256,
        /// The per-NO input quantity.
        amount: Decimal,
    },
}

impl ManagedPositionOperationRequest {
    /// Returns the operation type implied by the request.
    #[must_use]
    pub const fn operation(&self) -> PositionOperationType {
        match self {
            Self::Split { .. } => PositionOperationType::Split,
            Self::Merge { .. } => PositionOperationType::Merge,
            Self::RedeemCtf { .. } | Self::RedeemV2 { .. } => PositionOperationType::Redeem,
            Self::Convert { .. } => PositionOperationType::Convert,
        }
    }

    /// Validates the request against the managed intent.
    ///
    /// # Errors
    ///
    /// Returns an error if the intent operation, quantity, condition, or market does not match
    /// the request parameters.
    fn validate_intent(&self, intent: &ManagedOperationIntent) -> Result<()> {
        if intent.operation != self.operation() {
            return Err(Error::bad_request(format!(
                "managed operation {} does not match request type {}",
                intent.operation,
                self.operation()
            )));
        }

        let (quantity, condition_id, market_id) = match self {
            Self::Split {
                condition_id,
                amount,
            }
            | Self::Merge {
                condition_id,
                amount,
            } => (Some(*amount), Some(condition_id.as_str()), None),
            Self::RedeemCtf { condition_id } => (None, Some(condition_id.as_str()), None),
            Self::RedeemV2 {
                condition_id,
                amount,
                ..
            } => (Some(*amount), Some(condition_id.as_str()), None),
            Self::Convert {
                market_id, amount, ..
            } => (Some(*amount), None, Some(market_id.as_str())),
        };

        if let Some(quantity) = quantity
            && intent.quantity.map(|q| q.as_decimal()) != Some(quantity)
        {
            return Err(Error::bad_request(
                "managed operation intent quantity does not match the request",
            ));
        }

        if let Some(condition_id) = condition_id
            && let Some(expected) = intent.condition_id.as_deref()
            && expected != condition_id
        {
            return Err(Error::bad_request(
                "managed operation intent condition does not match the request",
            ));
        }

        if let Some(market_id) = market_id
            && let Some(expected) = intent.market_id.as_deref()
            && expected != market_id
        {
            return Err(Error::bad_request(
                "managed operation intent market does not match the request",
            ));
        }

        Ok(())
    }
}

/// Terminal result of a Polymarket position operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolymarketPositionOutcome {
    /// Relayer reported `STATE_CONFIRMED`.
    Confirmed {
        /// Relayer transaction identifier.
        transaction_id: String,
        /// On-chain transaction hash when the Relayer supplied one.
        transaction_hash: Option<String>,
    },
    /// Relayer reported `STATE_FAILED`.
    Failed {
        /// Relayer transaction identifier.
        transaction_id: String,
        /// On-chain transaction hash when the Relayer supplied one.
        transaction_hash: Option<String>,
        /// Relayer error detail when present.
        error_msg: Option<String>,
    },
    /// Relayer reported `STATE_INVALID`.
    Invalid {
        /// Relayer transaction identifier.
        transaction_id: String,
        /// Relayer error detail when present.
        error_msg: Option<String>,
    },
}

/// Submitted position operation that can be polled to a terminal Relayer state.
#[derive(Debug)]
pub struct PolymarketPositionTransaction {
    relayer: PolymarketRelayerHttpClient,
    transaction_id: String,
    wait_timeout: Duration,
    poll_interval: Duration,
}

impl PolymarketPositionTransaction {
    /// Relayer transaction identifier returned at submit time.
    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// Polls the Relayer until the transaction is confirmed, failed, or invalid.
    ///
    /// # Errors
    ///
    /// Returns an error if polling fails with a non-retryable status or the wait
    /// times out before a terminal state. A timeout leaves the on-chain outcome
    /// unknown.
    pub async fn wait(self) -> Result<PolymarketPositionOutcome> {
        let deadline = Instant::now() + self.wait_timeout;

        loop {
            match self.relayer.get_transaction(&self.transaction_id).await {
                Ok(tx) => {
                    if tx.state.is_terminal() {
                        return outcome_from_transaction(tx);
                    }
                }
                Err(e) if e.is_retryable() => {
                    log::warn!(
                        "Relayer transaction {} poll failed: {e}; retrying",
                        self.transaction_id
                    );
                }
                Err(e) => return Err(e),
            }

            if Instant::now() >= deadline {
                return Err(Error::exchange(format!(
                    "Relayer transaction {} wait timed out; terminal state is unknown",
                    self.transaction_id
                )));
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

/// Deposit Wallet client for split, merge, and redeem position operations.
#[derive(Debug)]
pub struct PolymarketPositionClient {
    signer: OrderSigner,
    deposit_wallet: Address,
    relayer: PolymarketRelayerHttpClient,
    clob: PolymarketClobPublicClient,
    clock: &'static AtomicTime,
    deadline_secs: u64,
    wait_timeout: Duration,
    poll_interval: Duration,
    wallet: wallet::WalletVerifier,
    submission: Arc<WalletSubmissionState>,
    recorder: Option<Arc<dyn PositionOperationRecorder>>,
    record_gate: Option<Arc<dyn record::PositionOperationRecordGate>>,
    record_store: Option<Arc<dyn PositionOperationRecordStore>>,
}

impl PolymarketPositionClient {
    /// Creates a Deposit Wallet position client.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are invalid, the deposit wallet is not
    /// distinct from the signer, or an HTTP client cannot be created.
    pub fn new(
        private_key: &EvmPrivateKey,
        deposit_wallet: &str,
        relayer_api_key: RelayerApiKey,
        base_url_relayer: Option<String>,
        base_url_clob: Option<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<ProxyUrl>,
    ) -> Result<Self> {
        let timeout_secs = timeout_secs.unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS);
        let signer = OrderSigner::new(private_key)?;
        let deposit_wallet = parse_address(deposit_wallet, "deposit_wallet")?;
        if deposit_wallet == signer.address() {
            return Err(Error::bad_request(
                "Deposit Wallet operations require a funder distinct from the signing address",
            ));
        }

        let relayer = PolymarketRelayerHttpClient::new_with_proxy(
            relayer_api_key,
            base_url_relayer,
            timeout_secs,
            proxy_url.clone(),
        )
        .map_err(Error::from_http_client)?;
        let wallet = wallet::WalletVerifier::new(timeout_secs, proxy_url.clone())?;
        let clob =
            PolymarketClobPublicClient::new_with_proxy(base_url_clob, timeout_secs, proxy_url)
                .map_err(Error::from_http_client)?;

        Ok(Self {
            signer,
            deposit_wallet,
            relayer,
            clob,
            clock: get_atomic_clock_realtime(),
            deadline_secs: DEFAULT_DEADLINE_SECS,
            wait_timeout: Duration::from_secs(DEFAULT_WAIT_TIMEOUT_SECS),
            poll_interval: Duration::from_millis(DEFAULT_POLL_INTERVAL_MS),
            wallet,
            submission: wallet_submission(deposit_wallet),
            recorder: Some(Arc::new(PositionOperationStoreRecorder::new(Arc::new(
                InMemoryPositionOperationStore::new(),
            )))),
            record_gate: None,
            record_store: None,
        })
    }

    /// Resumes polling a previously submitted Relayer transaction.
    #[must_use]
    pub fn resume_transaction(&self, transaction_id: &str) -> PolymarketPositionTransaction {
        PolymarketPositionTransaction {
            relayer: self.relayer.clone(),
            transaction_id: transaction_id.to_string(),
            wait_timeout: self.wait_timeout,
            poll_interval: self.poll_interval,
        }
    }

    /// Restores a wallet nonce reservation retained by a recovered operation record.
    ///
    /// A reservation is only installed when the client has no newer submission state, so a
    /// recovered unknown or pending signature keeps blocking further wallet operations.
    pub fn restore_submission(&self, nonce: U256, transaction_id: Option<String>) {
        match self.submission.try_lock() {
            Ok(mut submission) => {
                if submission.is_none() {
                    *submission = Some(WalletSubmission {
                        nonce,
                        transaction_id,
                    });
                }
            }
            Err(_) => log::warn!(
                "Deposit Wallet submission state is in use; recovered nonce reservation was not restored"
            ),
        }
    }

    /// Installs a recorder for managed operation preparation and submission notifications.
    #[must_use]
    pub fn with_recorder(mut self, recorder: Arc<dyn PositionOperationRecorder>) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Installs the asynchronous record gate used before signing managed operations.
    #[must_use]
    pub fn with_record_gate(mut self, gate: Arc<dyn record::PositionOperationRecordGate>) -> Self {
        self.record_gate = Some(gate);
        self
    }

    /// Installs the store used to retain operation records for recovery.
    #[must_use]
    pub fn with_record_store(mut self, store: Arc<dyn PositionOperationRecordStore>) -> Self {
        self.record_store = Some(store);
        self
    }

    /// Restores operation records loaded from persistent storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured record store rejects the recovered records.
    pub fn restore_position_operation_records(
        &self,
        records: Vec<PositionOperationRecord>,
    ) -> anyhow::Result<()> {
        let Some(store) = self.record_store.as_ref() else {
            return Ok(());
        };
        store.restore(records)
    }

    /// Returns the retained operation records for restart recovery.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured record store cannot be read.
    pub fn position_operation_records(&self) -> anyhow::Result<Vec<PositionOperationRecord>> {
        let Some(store) = self.record_store.as_ref() else {
            return Ok(Vec::new());
        };
        store.records()
    }

    /// Returns the currently known record for an operation, when retained by the recorder.
    #[must_use]
    pub fn position_operation_record(
        &self,
        operation_id: &UUID4,
    ) -> Option<PositionOperationRecord> {
        self.recorder.as_ref()?.load(operation_id)
    }

    /// Records the position legs the core applied for an operation.
    pub fn record_operation_applied(
        &self,
        operation_id: UUID4,
        legs: &[PositionOperationLeg],
        transaction_hash: Option<&str>,
    ) {
        let Some(recorder) = self.recorder.as_ref() else {
            return;
        };

        let applied = legs
            .iter()
            .map(|leg| PositionOperationAppliedLeg {
                position_id: leg.position_id,
                instrument_id: leg.instrument_id,
                strategy_id: Some(leg.strategy_id),
                direction: leg.direction,
                quantity: leg.quantity,
                cost: leg.cost,
                income: leg.income,
                settled: leg.settled,
            })
            .collect::<Vec<_>>();
        recorder.on_applied(
            operation_id,
            &applied,
            transaction_hash,
            self.clock.get_time_ns(),
        );
    }

    /// Records a recovery outcome that remains unresolved and blocking.
    pub fn record_recovery_unresolved(
        &self,
        operation_id: UUID4,
        venue_status: PositionOperationStatus,
        transaction_id: Option<&str>,
        transaction_hash: Option<&str>,
        message: &str,
    ) {
        let Some(recorder) = self.recorder.as_ref() else {
            return;
        };
        let venue_status = match venue_status {
            PositionOperationStatus::Confirmed | PositionOperationStatus::Applied => {
                record::PositionOperationVenueStatus::Confirmed
            }
            PositionOperationStatus::Submitted => record::PositionOperationVenueStatus::Submitted,
            PositionOperationStatus::Unknown => record::PositionOperationVenueStatus::Unknown,
            PositionOperationStatus::Failed | PositionOperationStatus::Rejected => {
                record::PositionOperationVenueStatus::Failed
            }
        };
        recorder.on_unresolved(
            operation_id,
            venue_status,
            transaction_id,
            transaction_hash,
            message,
            self.clock.get_time_ns(),
        );
    }

    /// Records a definitive terminal recovery failure.
    pub fn record_recovery_failed(&self, operation_id: UUID4, message: &str) {
        let Some(recorder) = self.recorder.as_ref() else {
            return;
        };
        recorder.on_submission_failed(operation_id, message, false, self.clock.get_time_ns());
    }

    /// Sets the Polygon RPC URL used to verify the Deposit Wallet before signing.
    #[must_use]
    pub fn with_rpc_url(mut self, rpc_url: String) -> Self {
        self.wallet.set_rpc_url(rpc_url);
        self
    }

    /// Sets the signed-batch deadline in seconds from now.
    ///
    /// Defaults to 1,800 seconds; values below one second are raised to one.
    /// Longer deadlines extend the period in which a signed batch can execute and
    /// delay expiry-based recovery of an unknown submission. The wait timeout does
    /// not shorten this deadline or cancel the signed batch.
    #[must_use]
    pub fn with_deadline_secs(mut self, deadline_secs: u64) -> Self {
        self.deadline_secs = deadline_secs.max(1);
        self
    }

    /// Sets how long [`PolymarketPositionTransaction::wait`] polls before timing out.
    #[must_use]
    pub fn with_wait_timeout(mut self, wait_timeout: Duration) -> Self {
        self.wait_timeout = wait_timeout;
        self
    }

    /// Sets the Relayer poll interval used by [`PolymarketPositionTransaction::wait`].
    #[must_use]
    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    /// Splits `amount` pUSD into a complete set of outcome tokens.
    ///
    /// # Errors
    ///
    /// Returns an error if market metadata or encoding is invalid, V2 balance or Router permission
    /// checks fail, or Relayer submission is rejected or ambiguous.
    pub async fn split_position(
        &self,
        condition_id: &str,
        amount: Decimal,
    ) -> Result<PolymarketPositionTransaction> {
        let call = self.prepare_split(condition_id, amount).await?;
        self.submit_call(call, "Split position").await
    }

    async fn prepare_split(&self, condition_id: &str, amount: Decimal) -> Result<PositionCall> {
        let base_amount = pusd_to_base_units(amount)?;

        Ok(match self.market(condition_id).await? {
            PositionMarket::Ctf { neg_risk } => {
                encode_split_position(condition_id, amount, neg_risk)?
            }
            PositionMarket::V2 { .. } => {
                let call = encode_split_position_v2(condition_id, amount)?;
                self.wallet
                    .verify_v2_split(self.deposit_wallet, base_amount)
                    .await?;
                call
            }
        })
    }

    /// Merges `amount` complete sets of outcome tokens back into pUSD.
    ///
    /// # Errors
    ///
    /// Returns an error if market metadata or encoding is invalid, V2 balance or Router permission
    /// checks fail, or Relayer submission is rejected or ambiguous.
    pub async fn merge_positions(
        &self,
        condition_id: &str,
        amount: Decimal,
    ) -> Result<PolymarketPositionTransaction> {
        let call = self.prepare_merge(condition_id, amount).await?;
        self.submit_call(call, "Merge positions").await
    }

    async fn prepare_merge(&self, condition_id: &str, amount: Decimal) -> Result<PositionCall> {
        let base_amount = pusd_to_base_units(amount)?;

        Ok(match self.market(condition_id).await? {
            PositionMarket::Ctf { neg_risk } => {
                encode_merge_positions(condition_id, amount, neg_risk)?
            }
            PositionMarket::V2 { position_ids } => {
                let call = encode_merge_positions_v2(condition_id, amount)?;
                self.wallet
                    .verify_v2_positions(self.deposit_wallet, &position_ids, base_amount)
                    .await?;
                call
            }
        })
    }

    /// Converts selected NO positions of a negative-risk CTF event.
    ///
    /// Bit `i` of `index_set` selects question index `i` as an input NO position. The adapter
    /// converts `amount` of each selected NO position into the unselected YES positions and any
    /// net pUSD collateral. The caller must already hold the NO positions and have approved the
    /// neg-risk collateral adapter as a Conditional Tokens operator.
    ///
    /// # Errors
    ///
    /// Returns an error if the market is not a known neg-risk CTF event, the index set is empty or
    /// out of range, metadata or on-chain reads fail, the operator approval or NO balances are
    /// insufficient, or Relayer submission is rejected or ambiguous.
    pub async fn convert_positions(
        &self,
        market_id: &str,
        index_set: U256,
        amount: Decimal,
    ) -> Result<PolymarketPositionTransaction> {
        let call = self.prepare_convert(market_id, index_set, amount).await?;
        self.submit_call(call, "Convert positions").await
    }

    async fn prepare_convert(
        &self,
        market_id: &str,
        index_set: U256,
        amount: Decimal,
    ) -> Result<PositionCall> {
        let parsed_market_id = parse_bytes32(market_id, "market_id")?;
        validate_market_id(parsed_market_id)?;
        let index_set = ConvertIndexSet::new(index_set)?;
        let base_amount = pusd_to_base_units(amount)?;
        let market = self.wallet.neg_risk_market(parsed_market_id).await?;
        index_set.validate(market.question_count)?;
        self.wallet
            .verify_convert(self.deposit_wallet, &market, index_set, base_amount)
            .await?;

        encode_convert_positions(market_id, index_set.bits(), amount)
    }

    /// Fetches the verified on-chain token effects of a confirmed transaction.
    ///
    /// Effects include outcome-token transfers between the Deposit Wallet and canonical
    /// contracts plus the net pUSD collateral change. Adapter or Router business events are
    /// not counted as transfers.
    ///
    /// # Errors
    ///
    /// Returns an error if the hash is invalid, the transaction is not yet mined, the receipt is
    /// malformed or reverted, or a Polygon RPC read fails.
    pub async fn transaction_effects(
        &self,
        transaction_hash: &str,
    ) -> Result<PositionOperationEffects> {
        let transaction_hash = parse_bytes32(transaction_hash, "transaction_hash")?;
        let receipt = self.wallet.transaction_receipt(transaction_hash).await?;
        PositionOperationEffects::parse(&receipt, self.deposit_wallet)
    }

    /// Loads verified on-chain metadata for a negative-risk event.
    ///
    /// # Errors
    ///
    /// Returns an error if the market ID is not canonical, the market is unknown, or a Polygon
    /// RPC read fails.
    pub async fn neg_risk_market(&self, market_id: &str) -> Result<NegRiskMarket> {
        let market_id = parse_bytes32(market_id, "market_id")?;
        validate_market_id(market_id)?;
        self.wallet.neg_risk_market(market_id).await
    }

    /// Loads the CTF payout denominator and the YES/NO payout numerators for a condition.
    ///
    /// # Errors
    ///
    /// Returns an error if the condition ID is invalid or a Polygon RPC read fails.
    pub async fn ctf_payouts(&self, condition_id: &str) -> Result<(U256, U256, U256)> {
        let condition_id = parse_bytes32(condition_id, "condition_id")?;
        let denominator = self.wallet.ctf_payout_denominator(condition_id).await?;
        let yes = self
            .wallet
            .ctf_payout_numerator(condition_id, U256::ZERO)
            .await?;
        let no = self
            .wallet
            .ctf_payout_numerator(condition_id, U256::ONE)
            .await?;
        Ok((denominator, yes, no))
    }

    /// Redeems both CTF binary outcome balances for a resolved market.
    ///
    /// # Errors
    ///
    /// Returns an error if the market is V2, metadata or encoding is invalid, or Relayer submission
    /// is rejected or ambiguous.
    pub async fn redeem_positions(
        &self,
        condition_id: &str,
    ) -> Result<PolymarketPositionTransaction> {
        let call = self.prepare_redeem_ctf(condition_id).await?;
        self.submit_call(call, "Redeem positions").await
    }

    async fn prepare_redeem_ctf(&self, condition_id: &str) -> Result<PositionCall> {
        let PositionMarket::Ctf { neg_risk } = self.market(condition_id).await? else {
            return Err(Error::bad_request(
                "redeem_positions supports only v1 CTF markets; use redeem_position for V2",
            ));
        };

        encode_redeem_positions(condition_id, neg_risk)
    }

    /// Redeems an explicit amount of V2 outcome index `0` or `1`.
    ///
    /// The index is the position ID's final byte, independent of CLOB token-array order.
    /// Amounts are exact six-decimal share quantities. A zero losing payout is redeemable;
    /// a reverted or unavailable payout read fails before submission.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is unsupported, amounts or outcomes are invalid, Router permissions
    /// are missing, balances are insufficient, payout reads fail, or submission is rejected or ambiguous.
    pub async fn redeem_position(
        &self,
        condition_id: &str,
        outcome_index: u8,
        amount: Decimal,
    ) -> Result<PolymarketPositionTransaction> {
        let call = self
            .prepare_redeem_v2(condition_id, outcome_index, amount)
            .await?;
        self.submit_call(call, "Redeem V2 position").await
    }

    async fn prepare_redeem_v2(
        &self,
        condition_id: &str,
        outcome_index: u8,
        amount: Decimal,
    ) -> Result<PositionCall> {
        let call = encode_redeem_position_v2(condition_id, outcome_index, amount)?;
        let base_amount = pusd_to_base_units(amount)?;

        let PositionMarket::V2 { position_ids, .. } = self.market(condition_id).await? else {
            return Err(Error::bad_request(
                "redeem_position requires a V2 market; use redeem_positions for CTF",
            ));
        };

        let position_id = position_ids[usize::from(outcome_index)];
        self.wallet
            .verify_v2_positions(self.deposit_wallet, &[position_id], base_amount)
            .await?;
        self.wallet.payout(position_id, base_amount).await?;
        Ok(call)
    }

    /// Approves ExchangeV3 trading from the Deposit Wallet holding the assets.
    ///
    /// `collateral_limit` is an explicit pUSD limit covering BUY spend plus fees. The same batch
    /// enables ExchangeV3 to transfer V2 positions for SELL orders. It does not redeem positions.
    /// The Relayer requires unlimited ExchangeV3 approvals and rejects this bounded request.
    /// V2 trading from a Deposit Wallet therefore requires existing approvals.
    ///
    /// # Errors
    ///
    /// Returns an error if the limit is invalid, wallet verification fails, or Relayer submission
    /// is rejected or ambiguous. A pending wallet submission must be reconciled before retrying.
    pub async fn approve_v2_trading(
        &self,
        collateral_limit: Decimal,
    ) -> Result<PolymarketPositionTransaction> {
        let amount = pusd_to_base_units(collateral_limit)?;
        let calls = approval_plan_v2(amount)
            .into_iter()
            .map(encode_approval)
            .collect::<Vec<_>>();
        self.submit_calls(&calls, "Approve V2 trading").await
    }

    async fn market(&self, condition_id: &str) -> Result<PositionMarket> {
        let query = if condition_id.len() == 64 {
            let native = parse_condition_id_v2(condition_id)?;
            format!("{native:#x}00")
        } else {
            condition_id.to_string()
        };

        let market = self.clob.get_market(&query).await?;
        let version = validate_data_market_version(market.version.as_deref())
            .map_err(|e| Error::bad_request(e.to_string()))?;

        let neg_risk = market.neg_risk.ok_or_else(|| {
            Error::bad_request(format!(
                "market metadata for {condition_id} is missing neg_risk"
            ))
        })?;

        if version != MARKET_VERSION_V2 {
            if parse_bytes32(&market.condition_id, "market condition_id")?
                != parse_bytes32(&query, "condition_id")?
            {
                return Err(Error::bad_request(
                    "Market condition ID does not match the requested condition",
                ));
            }

            return Ok(PositionMarket::Ctf { neg_risk });
        }

        let requested = parse_condition_id_v2(condition_id)?;
        if parse_condition_id_v2(&market.condition_id)? != requested {
            return Err(Error::bad_request(
                "Market condition ID does not match the requested V2 condition",
            ));
        }

        if (requested[0] == 2) != neg_risk {
            return Err(Error::bad_request(
                "V2 module and neg_risk metadata disagree",
            ));
        }

        let ids: Vec<_> = market
            .tokens
            .iter()
            .map(|token| token.token_id.clone())
            .collect();
        let outcomes: Vec<_> = market
            .tokens
            .iter()
            .map(|token| token.outcome.clone())
            .collect();
        validate_outcome_ids(&ids)
            .and_then(|()| validate_outcomes(&outcomes))
            .map_err(|e| Error::bad_request(e.to_string()))?;
        let yes_id = U256::from_be_slice(requested.as_slice()) << 8;
        let position_ids = [yes_id, yes_id + U256::from(1u8)];

        for id in ids {
            let id =
                U256::from_str_radix(&id, 10).map_err(|e| Error::bad_request(e.to_string()))?;
            if !position_ids.contains(&id) {
                return Err(Error::bad_request(
                    "V2 position IDs do not match the requested condition and outcomes",
                ));
            }
        }

        Ok(PositionMarket::V2 { position_ids })
    }

    /// Submits a managed position operation with preparation and result notifications.
    ///
    /// The intent is recorded with the unsigned nonce, deadline, and calldata before signing.
    /// A recorder failure aborts before signing or sending.
    ///
    /// # Errors
    ///
    /// Returns an error if the intent does not match the request, preparation fails, the recorder
    /// rejects the prepared record, or Relayer submission is rejected or ambiguous.
    pub async fn submit_managed_position_operation(
        &self,
        intent: ManagedOperationIntent,
        request: ManagedPositionOperationRequest,
    ) -> Result<PolymarketPositionTransaction> {
        request.validate_intent(&intent)?;

        let (call, metadata) = match &request {
            ManagedPositionOperationRequest::Split {
                condition_id,
                amount,
            } => (
                self.prepare_split(condition_id, *amount).await?,
                "Split position",
            ),
            ManagedPositionOperationRequest::Merge {
                condition_id,
                amount,
            } => (
                self.prepare_merge(condition_id, *amount).await?,
                "Merge positions",
            ),
            ManagedPositionOperationRequest::RedeemCtf { condition_id } => (
                self.prepare_redeem_ctf(condition_id).await?,
                "Redeem positions",
            ),
            ManagedPositionOperationRequest::RedeemV2 {
                condition_id,
                outcome_index,
                amount,
            } => (
                self.prepare_redeem_v2(condition_id, *outcome_index, *amount)
                    .await?,
                "Redeem V2 position",
            ),
            ManagedPositionOperationRequest::Convert {
                market_id,
                index_set,
                amount,
            } => (
                self.prepare_convert(market_id, *index_set, *amount).await?,
                "Convert positions",
            ),
        };

        self.submit_call_managed(call, metadata, &intent).await
    }

    async fn submit_call(
        &self,
        call: PositionCall,
        metadata: &str,
    ) -> Result<PolymarketPositionTransaction> {
        self.submit_calls(std::slice::from_ref(&call), metadata)
            .await
    }

    async fn submit_call_managed(
        &self,
        call: PositionCall,
        metadata: &str,
        intent: &ManagedOperationIntent,
    ) -> Result<PolymarketPositionTransaction> {
        self.submit_calls_with_intent(std::slice::from_ref(&call), metadata, Some(intent))
            .await
    }

    async fn submit_calls(
        &self,
        calls: &[PositionCall],
        metadata: &str,
    ) -> Result<PolymarketPositionTransaction> {
        self.submit_calls_with_intent(calls, metadata, None).await
    }

    async fn submit_calls_with_intent(
        &self,
        calls: &[PositionCall],
        metadata: &str,
        intent: Option<&ManagedOperationIntent>,
    ) -> Result<PolymarketPositionTransaction> {
        let mut submission = self.submission.lock().await;
        if let Some(previous) = submission.as_ref() {
            let Some(transaction_id) = previous.transaction_id.as_deref() else {
                return Err(Error::exchange(
                    "Previous Deposit Wallet submit outcome is unknown; reconcile it before further operations",
                ));
            };

            let transaction = self.relayer.get_transaction(transaction_id).await?;
            if !transaction.state.is_terminal() {
                return Err(Error::exchange(format!(
                    "Deposit Wallet transaction {transaction_id} is still pending"
                )));
            }
        }

        let nonce = self
            .wallet
            .verify(self.signer.address(), self.deposit_wallet)
            .await?;
        let now = U256::from(self.clock.get_time_ns().as_u64() / 1_000_000_000);

        if let Some(previous) = submission.as_ref()
            && nonce <= previous.nonce
        {
            return Err(Error::exchange(
                "Deposit Wallet nonce has not advanced; reconcile the previous submission before further operations",
            ));
        }

        let deadline = now.saturating_add(U256::from(self.deadline_secs));

        let wallet_calls: Vec<_> = calls
            .iter()
            .map(|call| DepositWalletCall {
                target: call.target,
                value: U256::ZERO,
                data: call.data.clone(),
            })
            .collect();

        if let Some(intent) = intent {
            let record = PositionOperationRecord::prepared(
                intent,
                &format!("{:#x}", self.deposit_wallet),
                &nonce.to_string(),
                &deadline.to_string(),
                wallet_calls
                    .iter()
                    .map(|call| PositionUnsignedCall {
                        target: format!("{:#x}", call.target),
                        value: call.value.to_string(),
                        data: format!("0x{}", alloy_primitives::hex::encode(&call.data)),
                    })
                    .collect(),
                self.clock.get_time_ns(),
            );

            if let Some(gate) = &self.record_gate {
                gate.confirm_prepared(&record).await.map_err(|e| {
                    Error::exchange(format!(
                        "Position operation record failed; submission aborted: {e}"
                    ))
                })?;
                // The core record is authoritative; the in-process copy only supports queries.
                if let Some(recorder) = &self.recorder
                    && let Err(e) = recorder.on_prepared(&record)
                {
                    log::warn!(
                        "Failed to mirror position operation {} in memory: {e}",
                        record.operation_id
                    );
                }
            } else if let Some(recorder) = &self.recorder {
                recorder.on_prepared(&record).map_err(|e| {
                    Error::exchange(format!(
                        "Position operation record failed; submission aborted: {e}"
                    ))
                })?;
            }
        }

        let signature = self.signer.sign_deposit_wallet_batch(
            self.deposit_wallet,
            nonce,
            deadline,
            &wallet_calls,
        )?;
        *submission = Some(WalletSubmission {
            nonce,
            transaction_id: None,
        });

        for wallet_call in &wallet_calls {
            log::info!(
                "Deposit Wallet submission: wallet={:#x}, nonce={nonce}, deadline={deadline}, operation={metadata}, target={:#x}, value={}, data={}",
                self.deposit_wallet,
                wallet_call.target,
                wallet_call.value,
                wallet_call.data,
            );
        }

        let submitted = match self
            .relayer
            .submit_wallet_batch(RelayerWalletSubmit {
                signer: self.signer.address(),
                deposit_wallet: self.deposit_wallet,
                nonce,
                deadline,
                signature: &signature,
                metadata,
                calls: &wallet_calls,
            })
            .await
        {
            Ok(submitted) => submitted,
            Err(e) => {
                if let (Some(recorder), Some(intent)) = (self.recorder.as_ref(), intent) {
                    recorder.on_submission_failed(
                        intent.operation_id,
                        &e.to_string(),
                        is_ambiguous_submit_error(&e),
                        self.clock.get_time_ns(),
                    );
                }
                return Err(e);
            }
        };

        let Some(transaction_id) = submitted.transaction_id else {
            if let (Some(recorder), Some(intent)) = (self.recorder.as_ref(), intent) {
                recorder.on_submission_failed(
                    intent.operation_id,
                    "Relayer submit response omitted transaction_id",
                    true,
                    self.clock.get_time_ns(),
                );
            }
            return Err(Error::decode(
                "Relayer submit response omitted transaction_id; transaction outcome is unknown",
            ));
        };

        if let (Some(recorder), Some(intent)) = (self.recorder.as_ref(), intent) {
            recorder.on_submitted(
                intent.operation_id,
                &transaction_id,
                self.clock.get_time_ns(),
            );
        }

        *submission = Some(WalletSubmission {
            nonce,
            transaction_id: Some(transaction_id.clone()),
        });

        Ok(PolymarketPositionTransaction {
            relayer: self.relayer.clone(),
            transaction_id,
            wait_timeout: self.wait_timeout,
            poll_interval: self.poll_interval,
        })
    }
}

enum PositionMarket {
    Ctf { neg_risk: bool },
    V2 { position_ids: [U256; 2] },
}

#[derive(Debug)]
struct WalletSubmission {
    nonce: U256,
    transaction_id: Option<String>,
}

type WalletSubmissionState = tokio::sync::Mutex<Option<WalletSubmission>>;

fn wallet_submission(wallet: Address) -> Arc<WalletSubmissionState> {
    let mut submissions = WALLET_SUBMISSIONS.lock();
    submissions.retain(|_, state| {
        Arc::strong_count(state) > 1 || state.try_lock().map_or(true, |state| state.is_some())
    });

    submissions.entry(wallet).or_default().clone()
}

pub(crate) fn is_ambiguous_submit_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Transport(_) | Error::Timeout | Error::Decode(_)
    )
}

fn outcome_from_transaction(tx: RelayerTransaction) -> Result<PolymarketPositionOutcome> {
    let transaction_id = tx.transaction_id.clone().ok_or_else(|| {
        Error::decode(
            "Relayer terminal response omitted transaction_id; transaction outcome is unknown",
        )
    })?;

    match tx.state {
        RelayerTransactionState::Confirmed => Ok(PolymarketPositionOutcome::Confirmed {
            transaction_id,
            transaction_hash: tx.transaction_hash,
        }),
        RelayerTransactionState::Failed => Ok(PolymarketPositionOutcome::Failed {
            transaction_id,
            transaction_hash: tx.transaction_hash,
            error_msg: tx.error_msg,
        }),
        RelayerTransactionState::Invalid => Ok(PolymarketPositionOutcome::Invalid {
            transaction_id,
            error_msg: tx.error_msg,
        }),
        RelayerTransactionState::New | RelayerTransactionState::Other(_) => {
            Err(Error::decode(format!(
                "Relayer transaction {transaction_id} is not terminal, state was {}",
                tx.state.as_str()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UUID4;
    use nautilus_model::{
        identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
        position_operation::PositionOperationType,
        types::Quantity,
    };
    use nautilus_network::retry::RetryConfig;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[tokio::test]
    #[ignore = "requires read-only access to live Polymarket Gamma and CLOB APIs"]
    async fn test_live_market_protocol_selection() {
        let key = EvmPrivateKey::new(
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
        )
        .unwrap();
        let relayer_key = RelayerApiKey::new(
            "unused".into(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
        )
        .unwrap();
        let client = PolymarketPositionClient::new(
            &key,
            "0x1111111111111111111111111111111111111111",
            relayer_key,
            Some("http://127.0.0.1:1".into()),
            None,
            Some(20),
            None,
        )
        .unwrap();
        let gamma =
            crate::http::gamma::PolymarketGammaHttpClient::new(None, 20, RetryConfig::default())
                .unwrap();

        for (market_id, protocol) in [("1557558", "v1"), ("5931597", "v2")] {
            let markets = vec![gamma.inner().get_gamma_market(market_id).await.unwrap()];
            let market = &markets[0];
            assert_eq!(market.version.as_deref(), Some(protocol));
            let (instruments, transient) =
                crate::http::gamma::parse_markets_with_transient(&markets, 1.into());
            assert_eq!(instruments.len(), 2);
            assert_eq!(transient, Vec::<String>::new());
            let result = client.market(&market.condition_id).await.unwrap();

            if protocol == "v1" {
                assert!(
                    matches!(result, PositionMarket::Ctf { neg_risk } if neg_risk == market.neg_risk.unwrap())
                );
            } else {
                assert!(matches!(result, PositionMarket::V2 { .. }));
            }
        }
    }

    #[rstest]
    fn test_managed_request_requires_matching_intent() {
        let operation_id = UUID4::from("00000000-0000-4000-8000-000000000001");
        let intent = ManagedOperationIntent {
            operation_id,
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            account_id: AccountId::from("POLYMARKET-001"),
            operation: PositionOperationType::Split,
            instrument_ids: vec![InstrumentId::from("YES-TOKEN.POLYMARKET")],
            barrier_instrument_ids: vec![InstrumentId::from("YES-TOKEN.POLYMARKET")],
            quantity: Some(Quantity::from("100")),
            condition_id: Some("0xcondition".into()),
            market_id: None,
            index_set: None,
        };

        let request = ManagedPositionOperationRequest::Split {
            condition_id: "0xcondition".into(),
            amount: dec!(100),
        };
        assert!(request.validate_intent(&intent).is_ok());

        let wrong_operation = ManagedPositionOperationRequest::Merge {
            condition_id: "0xcondition".into(),
            amount: dec!(100),
        };
        assert!(wrong_operation.validate_intent(&intent).is_err());

        let wrong_quantity = ManagedPositionOperationRequest::Split {
            condition_id: "0xcondition".into(),
            amount: dec!(50),
        };
        assert!(wrong_quantity.validate_intent(&intent).is_err());

        let wrong_condition = ManagedPositionOperationRequest::Split {
            condition_id: "0xother".into(),
            amount: dec!(100),
        };
        assert!(wrong_condition.validate_intent(&intent).is_err());
    }

    #[rstest]
    fn test_managed_convert_request_checks_market() {
        let intent = ManagedOperationIntent {
            operation_id: UUID4::from("00000000-0000-4000-8000-000000000002"),
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            account_id: AccountId::from("POLYMARKET-001"),
            operation: PositionOperationType::Convert,
            instrument_ids: vec![InstrumentId::from("NO-TOKEN.POLYMARKET")],
            barrier_instrument_ids: vec![InstrumentId::from("NO-TOKEN.POLYMARKET")],
            quantity: Some(Quantity::from("100")),
            condition_id: None,
            market_id: Some("0xmarket".into()),
            index_set: Some("1".into()),
        };

        assert!(
            ManagedPositionOperationRequest::Convert {
                market_id: "0xmarket".into(),
                index_set: U256::ONE,
                amount: dec!(100),
            }
            .validate_intent(&intent)
            .is_ok()
        );
        assert!(
            ManagedPositionOperationRequest::Convert {
                market_id: "0xother".into(),
                index_set: U256::ONE,
                amount: dec!(100),
            }
            .validate_intent(&intent)
            .is_err()
        );
    }

    #[rstest]
    fn test_outcome_from_confirmed_transaction() {
        let outcome = outcome_from_transaction(RelayerTransaction {
            transaction_id: Some("tx-1".into()),
            transaction_hash: Some("0xabc".into()),
            state: RelayerTransactionState::Confirmed,
            error_msg: None,
        })
        .unwrap();

        assert_eq!(
            outcome,
            PolymarketPositionOutcome::Confirmed {
                transaction_id: "tx-1".into(),
                transaction_hash: Some("0xabc".into()),
            }
        );
    }

    #[rstest]
    fn test_outcome_from_failed_and_invalid_transactions() {
        let failed = outcome_from_transaction(RelayerTransaction {
            transaction_id: Some("tx-2".into()),
            transaction_hash: None,
            state: RelayerTransactionState::Failed,
            error_msg: Some("reverted".into()),
        })
        .unwrap();

        assert_eq!(
            failed,
            PolymarketPositionOutcome::Failed {
                transaction_id: "tx-2".into(),
                transaction_hash: None,
                error_msg: Some("reverted".into()),
            }
        );

        let invalid = outcome_from_transaction(RelayerTransaction {
            transaction_id: Some("tx-3".into()),
            transaction_hash: None,
            state: RelayerTransactionState::Invalid,
            error_msg: Some("bad nonce".into()),
        })
        .unwrap();

        assert_eq!(
            invalid,
            PolymarketPositionOutcome::Invalid {
                transaction_id: "tx-3".into(),
                error_msg: Some("bad nonce".into()),
            }
        );
    }

    #[rstest]
    fn test_outcome_from_non_terminal_is_error() {
        let err = outcome_from_transaction(RelayerTransaction {
            transaction_id: Some("tx-4".into()),
            transaction_hash: None,
            state: RelayerTransactionState::New,
            error_msg: None,
        })
        .unwrap_err();

        assert!(err.to_string().contains("is not terminal"));
        assert!(err.to_string().contains("state was STATE_NEW"));
    }
}
