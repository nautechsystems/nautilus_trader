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

mod wallet;

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use alloy_primitives::{Address, U256};
use nautilus_core::time::{AtomicTime, get_atomic_clock_realtime};
use nautilus_network::websocket::proxy::ProxyUrl;
use parking_lot::Mutex;
use rust_decimal::Decimal;

use self::calldata::{
    PositionCall, encode_merge_positions, encode_redeem_positions, encode_split_position,
};
use crate::{
    common::credential::{EvmPrivateKey, RelayerApiKey},
    http::{
        clob::PolymarketClobPublicClient,
        error::{Error, Result},
        relayer::{
            PolymarketRelayerHttpClient, RelayerTransaction, RelayerTransactionState,
            RelayerWalletSubmit,
        },
    },
    signing::eip712::{DepositWalletCall, OrderSigner, parse_address},
};

const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 60;
const DEFAULT_DEADLINE_SECS: u64 = 1_800;
const DEFAULT_WAIT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;

static WALLET_SUBMISSIONS: LazyLock<Mutex<HashMap<Address, Arc<WalletSubmissionState>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
        })
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
    /// Returns an error if market metadata is invalid, encoding fails, or Relayer
    /// submission is rejected or ambiguous.
    pub async fn split_position(
        &self,
        condition_id: &str,
        amount: Decimal,
    ) -> Result<PolymarketPositionTransaction> {
        let neg_risk = self.market_neg_risk(condition_id).await?;
        let call = encode_split_position(condition_id, amount, neg_risk)?;
        self.submit_call(call, "Split position").await
    }

    /// Merges `amount` complete sets of outcome tokens back into pUSD.
    ///
    /// # Errors
    ///
    /// Returns an error if market metadata is invalid, encoding fails, or Relayer
    /// submission is rejected or ambiguous.
    pub async fn merge_positions(
        &self,
        condition_id: &str,
        amount: Decimal,
    ) -> Result<PolymarketPositionTransaction> {
        let neg_risk = self.market_neg_risk(condition_id).await?;
        let call = encode_merge_positions(condition_id, amount, neg_risk)?;
        self.submit_call(call, "Merge positions").await
    }

    /// Redeems both binary outcome balances for a resolved market.
    ///
    /// # Errors
    ///
    /// Returns an error if market metadata is invalid, encoding fails, or Relayer
    /// submission is rejected or ambiguous.
    pub async fn redeem_positions(
        &self,
        condition_id: &str,
    ) -> Result<PolymarketPositionTransaction> {
        let neg_risk = self.market_neg_risk(condition_id).await?;
        let call = encode_redeem_positions(condition_id, neg_risk)?;
        self.submit_call(call, "Redeem positions").await
    }

    async fn market_neg_risk(&self, condition_id: &str) -> Result<bool> {
        let market = self.clob.get_market(condition_id).await?;
        market.neg_risk.ok_or_else(|| {
            Error::bad_request(format!(
                "market metadata for {condition_id} is missing neg_risk"
            ))
        })
    }

    async fn submit_call(
        &self,
        call: PositionCall,
        metadata: &str,
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

        let wallet_call = DepositWalletCall {
            target: call.target,
            value: U256::ZERO,
            data: call.data,
        };

        let signature = self.signer.sign_deposit_wallet_batch(
            self.deposit_wallet,
            nonce,
            deadline,
            std::slice::from_ref(&wallet_call),
        )?;
        *submission = Some(WalletSubmission {
            nonce,
            transaction_id: None,
        });

        log::info!(
            "Deposit Wallet submission: wallet={:#x}, nonce={nonce}, deadline={deadline}, operation={metadata}, target={:#x}, value={}, data={}",
            self.deposit_wallet,
            wallet_call.target,
            wallet_call.value,
            wallet_call.data,
        );

        let submitted = self
            .relayer
            .submit_wallet_batch(RelayerWalletSubmit {
                signer: self.signer.address(),
                deposit_wallet: self.deposit_wallet,
                nonce,
                deadline,
                signature: &signature,
                metadata,
                calls: std::slice::from_ref(&wallet_call),
            })
            .await?;

        let Some(transaction_id) = submitted.transaction_id else {
            return Err(Error::decode(
                "Relayer submit response omitted transaction_id; transaction outcome is unknown",
            ));
        };

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
    use rstest::rstest;

    use super::*;

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
