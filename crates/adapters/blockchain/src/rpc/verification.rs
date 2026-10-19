// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License. You may obtain a copy of the
//  License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under the
//  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
//  either express or implied. See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::{collections::HashSet, fmt::Debug, str::FromStr, sync::Arc};

use alloy::primitives::{Address, B256, Bytes, U256, keccak256};

use crate::{
    config::{
        BlockchainChainAnchorConfig, BlockchainContractRole, BlockchainDeploymentManifest,
        BlockchainProviderIdentity, BlockchainVerificationConfig,
        BlockchainVerificationProviderConfig,
    },
    contracts::uniswap_v3_quote::{
        UniswapV3Quote, decode_quote_exact_input_single, decode_quote_exact_output_single,
        quote_exact_input_single_call, quote_exact_output_single_call,
    },
    rpc::{
        http::{BlockchainHttpRpcClient, validate_execution_endpoint},
        types::{
            RpcBlock, RpcCallResult, RpcCallTrace, RpcCallType, RpcTransaction,
            RpcTransactionReceipt,
        },
    },
};

const VERIFICATION_PROVIDER_COUNT: usize = 2;
const VERIFICATION_SOURCE_COUNT: usize = 3;
pub(crate) const VERIFICATION_HEADER_WINDOW_MAX: u64 = 4_096;
pub(crate) const VERIFICATION_SCHEMA_VERSION: i16 = 2;

/// A security-critical read class retained with verification evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerificationRead {
    ChainId,
    LatestBlock,
    NumberedBlock,
    FinalizedBlock,
    DeploymentIdentity,
    ContractCode,
    ContractStorage,
    TransactionCount,
    PendingTransactionCount,
    Balance,
    ContractCall,
    Quote,
    GasEstimate,
    PriorityFee,
    Receipt,
    Transaction,
    ReplacementBlock,
    CallTrace,
}

/// A value authorized by all three configured sources and local anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verified<T> {
    pub value: T,
    pub read: VerificationRead,
    pub provider_ids: [String; VERIFICATION_SOURCE_COUNT],
    pub normalized_value_digest: B256,
}

/// Sanitized detail for a non-verified read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerificationFailure {
    pub read: VerificationRead,
    pub provider_ids: [String; VERIFICATION_SOURCE_COUNT],
}

/// Exhaustive result of a security-critical verification operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub(crate) enum VerificationOutcome<T> {
    Verified(Verified<T>),
    Disagreement(VerificationFailure),
    Unavailable(VerificationFailure),
    Retryable(VerificationFailure),
    LocallyInvalid(VerificationFailure),
}

/// Contract-specific result of a verified EVM simulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VerifiedSimulation<T> {
    Succeeded(T),
    Denied,
}

/// Block fields which must agree independently of provider-specific extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerifiedBlockHeader {
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub base_fee_per_gas: Option<u128>,
}

/// Stable semantic projection of one call-trace frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedCallTrace {
    pub call_type: RpcCallType,
    pub from: Address,
    pub to: Option<Address>,
    pub value: U256,
    pub input_selector: Option<[u8; 4]>,
    pub input_digest: B256,
    pub success: bool,
    pub calls: Vec<Self>,
}

/// Stable normalized receipt log used for exact agreement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedReceiptLog {
    pub removed: bool,
    pub log_index: Option<u64>,
    pub transaction_index: Option<u64>,
    pub transaction_hash: Option<B256>,
    pub block_hash: Option<B256>,
    pub block_number: Option<u64>,
    pub address: Address,
    pub data: Bytes,
    pub topics: Vec<B256>,
}

/// Receipt fields which must agree across all three sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedReceipt {
    pub transaction_hash: B256,
    pub block_hash: B256,
    pub block_number: u64,
    pub gas_used: u64,
    pub effective_gas_price: U256,
    pub transaction_index: u64,
    pub status: bool,
    pub logs: Vec<VerifiedReceiptLog>,
}

impl TryFrom<RpcTransactionReceipt> for VerifiedReceipt {
    type Error = anyhow::Error;

    fn try_from(receipt: RpcTransactionReceipt) -> Result<Self, Self::Error> {
        let logs = receipt
            .logs
            .into_iter()
            .map(|log| {
                Ok(VerifiedReceiptLog {
                    removed: log.removed,
                    log_index: parse_optional_quantity(log.log_index.as_deref())?,
                    transaction_index: parse_optional_quantity(log.transaction_index.as_deref())?,
                    transaction_hash: parse_optional_hash(log.transaction_hash.as_deref())?,
                    block_hash: parse_optional_hash(log.block_hash.as_deref())?,
                    block_number: parse_optional_quantity(log.block_number.as_deref())?,
                    address: Address::from_str(&log.address)
                        .map_err(|_| anyhow::anyhow!("Invalid receipt log address"))?,
                    data: Bytes::from_str(&log.data)
                        .map_err(|_| anyhow::anyhow!("Invalid receipt log data"))?,
                    topics: log
                        .topics
                        .iter()
                        .map(|topic| {
                            B256::from_str(topic)
                                .map_err(|_| anyhow::anyhow!("Invalid receipt log topic"))
                        })
                        .collect::<anyhow::Result<_>>()?,
                })
            })
            .collect::<anyhow::Result<_>>()?;
        Ok(Self {
            transaction_hash: receipt.transaction_hash,
            block_hash: receipt.block_hash,
            block_number: receipt.block_number,
            gas_used: receipt.gas_used,
            effective_gas_price: receipt.effective_gas_price,
            transaction_index: receipt.transaction_index,
            status: receipt.status,
            logs,
        })
    }
}

impl From<RpcCallTrace> for VerifiedCallTrace {
    fn from(trace: RpcCallTrace) -> Self {
        let input_selector = trace.input.get(..4).map(|selector| {
            let mut result = [0; 4];
            result.copy_from_slice(selector);
            result
        });
        Self {
            call_type: trace.call_type,
            from: trace.from,
            to: trace.to,
            value: trace.value,
            input_selector,
            input_digest: keccak256(&trace.input),
            success: trace.error.is_none(),
            calls: trace.calls.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<RpcBlock> for VerifiedBlockHeader {
    fn from(block: RpcBlock) -> Self {
        Self {
            number: block.number,
            hash: block.hash,
            parent_hash: block.parent_hash,
            timestamp: block.timestamp,
            base_fee_per_gas: block.base_fee_per_gas,
        }
    }
}

impl VerificationRead {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::ChainId => "chain_id",
            Self::LatestBlock => "latest_block",
            Self::NumberedBlock => "numbered_block",
            Self::FinalizedBlock => "finalized_block",
            Self::DeploymentIdentity => "deployment_identity",
            Self::ContractCode => "contract_code",
            Self::ContractStorage => "contract_storage",
            Self::TransactionCount => "transaction_count",
            Self::PendingTransactionCount => "pending_transaction_count",
            Self::Balance => "balance",
            Self::ContractCall => "contract_call",
            Self::Quote => "quote",
            Self::GasEstimate => "gas_estimate",
            Self::PriorityFee => "priority_fee",
            Self::Receipt => "receipt",
            Self::Transaction => "transaction",
            Self::ReplacementBlock => "replacement_block",
            Self::CallTrace => "call_trace",
        }
    }
}

/// A JSON-RPC client whose public surface contains no broadcast operation.
#[derive(Clone)]
pub(crate) struct VerificationRpcClient {
    identity: BlockchainProviderIdentity,
    rpc: Arc<BlockchainHttpRpcClient>,
}

impl Debug for VerificationRpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(VerificationRpcClient))
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl VerificationRpcClient {
    pub(crate) fn new(
        identity: BlockchainProviderIdentity,
        http_rpc_url: String,
        rpc_requests_per_second: Option<u32>,
    ) -> Self {
        Self {
            identity,
            rpc: Arc::new(BlockchainHttpRpcClient::new(
                http_rpc_url,
                rpc_requests_per_second,
                None,
            )),
        }
    }

    pub(crate) fn from_authoritative(
        identity: BlockchainProviderIdentity,
        rpc: Arc<BlockchainHttpRpcClient>,
    ) -> Self {
        Self { identity, rpc }
    }

    async fn chain_id(&self) -> anyhow::Result<u64> {
        self.rpc.chain_id().await
    }

    async fn latest_block(&self) -> anyhow::Result<RpcBlock> {
        self.rpc.latest_block().await
    }

    async fn finalized_block(&self) -> anyhow::Result<RpcBlock> {
        self.rpc.finalized_block().await
    }

    async fn block_by_number(&self, number: u64) -> anyhow::Result<RpcBlock> {
        self.rpc.block_by_number(number, false).await
    }

    async fn code_at(&self, address: &Address, block: u64) -> anyhow::Result<Bytes> {
        self.rpc.get_code_at(address, block).await
    }

    async fn storage_at(&self, address: &Address, slot: &B256, block: u64) -> anyhow::Result<B256> {
        self.rpc.get_storage_at(address, slot, block).await
    }

    async fn transaction_count_at(&self, address: &Address, block: u64) -> anyhow::Result<u64> {
        self.rpc.get_transaction_count_at(address, block).await
    }

    async fn transaction_count_pending(&self, address: &Address) -> anyhow::Result<u64> {
        self.rpc.get_transaction_count_pending(address).await
    }

    async fn balance_at(&self, address: &Address, block: u64) -> anyhow::Result<U256> {
        self.rpc.get_balance(address, Some(block)).await
    }

    async fn call_at(
        &self,
        from: Option<&Address>,
        to: &Address,
        value: U256,
        data: &[u8],
        block: u64,
    ) -> anyhow::Result<Bytes> {
        self.rpc.call_at(from, to, value, data, block).await
    }

    async fn call_result_at(
        &self,
        from: Option<&Address>,
        to: &Address,
        value: U256,
        data: &[u8],
        block: u64,
    ) -> anyhow::Result<RpcCallResult> {
        self.rpc.call_result_at(from, to, value, data, block).await
    }

    async fn estimate_gas_at(
        &self,
        from: &Address,
        to: &Address,
        value: U256,
        data: &[u8],
        block: u64,
    ) -> anyhow::Result<u64> {
        self.rpc.estimate_gas_at(from, to, value, data, block).await
    }

    async fn priority_fee(&self) -> anyhow::Result<u128> {
        self.rpc.max_priority_fee_per_gas().await
    }

    async fn receipt(&self, tx_hash: &B256) -> anyhow::Result<Option<RpcTransactionReceipt>> {
        self.rpc.get_transaction_receipt(tx_hash).await
    }

    async fn transaction(&self, tx_hash: &B256) -> anyhow::Result<Option<RpcTransaction>> {
        self.rpc.get_transaction_by_hash(tx_hash).await
    }

    async fn block_with_transactions(&self, number: u64) -> anyhow::Result<RpcBlock> {
        self.rpc.block_by_number(number, true).await
    }

    async fn call_trace(&self, tx_hash: &B256) -> anyhow::Result<RpcCallTrace> {
        self.rpc.trace_transaction_call(tx_hash).await
    }

    async fn probe_call_trace(&self) -> anyhow::Result<()> {
        self.rpc.probe_call_trace().await
    }
}

/// Coordinates class-specific reads across one authoritative source and two verifiers.
#[derive(Debug, Clone)]
pub(crate) struct VerificationCoordinator {
    sources: [VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
    anchor: BlockchainChainAnchorConfig,
}

impl VerificationCoordinator {
    /// Validates the topology and constructs read-only clients for all three sources.
    pub(crate) fn new(
        authoritative_rpc: Arc<BlockchainHttpRpcClient>,
        authoritative_url: &str,
        config: &BlockchainVerificationConfig,
        rpc_requests_per_second: Option<u32>,
    ) -> anyhow::Result<Self> {
        validate_config(authoritative_url, config)?;

        let authoritative = VerificationRpcClient::from_authoritative(
            config.authoritative.clone(),
            authoritative_rpc,
        );
        let mut verifiers = config.verifiers.iter().map(
            |BlockchainVerificationProviderConfig {
                 identity,
                 http_rpc_url,
             }| {
                VerificationRpcClient::new(
                    identity.clone(),
                    http_rpc_url.clone().into_inner(),
                    rpc_requests_per_second,
                )
            },
        );
        let verifier_a = verifiers.next().expect("validated verifier count");
        let verifier_b = verifiers.next().expect("validated verifier count");

        Ok(Self {
            sources: [authoritative, verifier_a, verifier_b],
            anchor: config.chain_anchor.clone(),
        })
    }

    pub(crate) async fn verify_chain_id(&self) -> VerificationOutcome<u64> {
        let (a, b, c) = tokio::join!(
            self.sources[0].chain_id(),
            self.sources[1].chain_id(),
            self.sources[2].chain_id(),
        );

        match classify_exact(VerificationRead::ChainId, [a, b, c], &self.sources) {
            VerificationOutcome::Verified(verified)
                if verified.value == u64::from(self.anchor.chain_id) =>
            {
                VerificationOutcome::Verified(verified)
            }
            VerificationOutcome::Verified(_) => {
                VerificationOutcome::Disagreement(self.failure(VerificationRead::ChainId))
            }
            other => other,
        }
    }

    pub(crate) async fn verify_block(
        &self,
        number: u64,
    ) -> VerificationOutcome<VerifiedBlockHeader> {
        let (a, b, c) = tokio::join!(
            self.sources[0].block_by_number(number),
            self.sources[1].block_by_number(number),
            self.sources[2].block_by_number(number),
        );
        classify_exact(
            VerificationRead::NumberedBlock,
            [a.map(Into::into), b.map(Into::into), c.map(Into::into)],
            &self.sources,
        )
    }

    pub(crate) async fn verify_checkpoint(&self) -> VerificationOutcome<VerifiedBlockHeader> {
        match self.verify_block(self.anchor.checkpoint_height).await {
            VerificationOutcome::Verified(verified)
                if verified.value.hash
                    == B256::from_str(&self.anchor.checkpoint_hash)
                        .expect("validated checkpoint hash")
                    && verified.value.timestamp == self.anchor.checkpoint_timestamp =>
            {
                VerificationOutcome::Verified(verified)
            }
            VerificationOutcome::Verified(_) => {
                VerificationOutcome::Disagreement(self.failure(VerificationRead::NumberedBlock))
            }
            other => other,
        }
    }

    pub(crate) async fn verify_finalized_header(&self) -> VerificationOutcome<VerifiedBlockHeader> {
        let (a, b, c) = tokio::join!(
            self.sources[0].finalized_block(),
            self.sources[1].finalized_block(),
            self.sources[2].finalized_block(),
        );
        let finalized =
            match collect_values(VerificationRead::FinalizedBlock, [a, b, c], &self.sources) {
                Ok(values) => values,
                Err(outcome) => return outcome,
            };

        if finalized
            .iter()
            .any(|block| block.number < self.anchor.checkpoint_height)
        {
            return VerificationOutcome::Disagreement(
                self.failure(VerificationRead::FinalizedBlock),
            );
        }
        let height = finalized.iter().map(|block| block.number).min().unwrap();
        match self.verify_block(height).await {
            VerificationOutcome::Verified(verified) => VerificationOutcome::Verified(Verified {
                value: verified.value,
                read: VerificationRead::FinalizedBlock,
                provider_ids: verified.provider_ids,
                normalized_value_digest: verified.normalized_value_digest,
            }),
            VerificationOutcome::Disagreement(failure) => {
                VerificationOutcome::Disagreement(failure)
            }
            VerificationOutcome::Unavailable(failure) => VerificationOutcome::Unavailable(failure),
            VerificationOutcome::Retryable(failure) => VerificationOutcome::Retryable(failure),
            VerificationOutcome::LocallyInvalid(failure) => {
                VerificationOutcome::LocallyInvalid(failure)
            }
        }
    }

    pub(crate) async fn verify_decision_header(
        &self,
        now_unix_secs: u64,
    ) -> VerificationOutcome<VerifiedBlockHeader> {
        let (a, b, c) = tokio::join!(
            self.sources[0].latest_block(),
            self.sources[1].latest_block(),
            self.sources[2].latest_block(),
        );
        let heads = match collect_values(VerificationRead::LatestBlock, [a, b, c], &self.sources) {
            Ok(values) => values,
            Err(outcome) => return outcome,
        };

        if heads
            .iter()
            .any(|block| block.number < self.anchor.checkpoint_height)
        {
            return VerificationOutcome::Disagreement(self.failure(VerificationRead::LatestBlock));
        }
        let min_height = heads.iter().map(|block| block.number).min().unwrap();
        let max_height = heads.iter().map(|block| block.number).max().unwrap();
        if max_height - min_height > self.anchor.max_head_skew_blocks {
            return VerificationOutcome::Unavailable(self.failure(VerificationRead::LatestBlock));
        }

        match self.verify_block(min_height).await {
            VerificationOutcome::Verified(verified)
                if verified.value.timestamp
                    > now_unix_secs.saturating_add(self.anchor.max_future_drift_secs) =>
            {
                VerificationOutcome::Disagreement(self.failure(VerificationRead::NumberedBlock))
            }
            VerificationOutcome::Verified(verified)
                if now_unix_secs.saturating_sub(verified.value.timestamp)
                    > self.anchor.max_head_age_secs =>
            {
                VerificationOutcome::Unavailable(self.failure(VerificationRead::NumberedBlock))
            }
            other => other,
        }
    }

    pub(crate) async fn verify_header_window(
        &self,
        parent: VerifiedBlockHeader,
        end_height: u64,
    ) -> VerificationOutcome<Vec<VerifiedBlockHeader>> {
        if end_height < parent.number
            || end_height.saturating_sub(parent.number) > VERIFICATION_HEADER_WINDOW_MAX
        {
            return VerificationOutcome::LocallyInvalid(
                self.failure(VerificationRead::NumberedBlock),
            );
        }

        let mut previous = parent;
        let mut headers = Vec::with_capacity((end_height - previous.number) as usize);
        for number in previous.number + 1..=end_height {
            match self.verify_block(number).await {
                VerificationOutcome::Verified(verified)
                    if verified.value.parent_hash == previous.hash =>
                {
                    previous = verified.value;
                    headers.push(previous);
                }
                VerificationOutcome::Verified(_) => {
                    return VerificationOutcome::Disagreement(
                        self.failure(VerificationRead::NumberedBlock),
                    );
                }
                VerificationOutcome::Disagreement(failure) => {
                    return VerificationOutcome::Disagreement(failure);
                }
                VerificationOutcome::Unavailable(failure) => {
                    return VerificationOutcome::Unavailable(failure);
                }
                VerificationOutcome::Retryable(failure) => {
                    return VerificationOutcome::Retryable(failure);
                }
                VerificationOutcome::LocallyInvalid(failure) => {
                    return VerificationOutcome::LocallyInvalid(failure);
                }
            }
        }

        VerificationOutcome::Verified(Verified {
            normalized_value_digest: normalized_digest(&headers),
            value: headers,
            read: VerificationRead::NumberedBlock,
            provider_ids: self.provider_ids(),
        })
    }

    pub(crate) async fn verify_code(
        &self,
        address: &Address,
        block: u64,
    ) -> VerificationOutcome<Bytes> {
        let (a, b, c) = tokio::join!(
            self.sources[0].code_at(address, block),
            self.sources[1].code_at(address, block),
            self.sources[2].code_at(address, block),
        );
        classify_exact(VerificationRead::ContractCode, [a, b, c], &self.sources)
    }

    pub(crate) async fn verify_deployment_manifest(
        &self,
        manifest: &BlockchainDeploymentManifest,
        block: u64,
    ) -> VerificationOutcome<()> {
        for contract in &manifest.contracts {
            let address = Address::from_str(&contract.address).expect("validated manifest address");
            let expected_hash =
                B256::from_str(&contract.runtime_code_hash).expect("validated manifest code hash");
            let code = match self.verify_code(&address, block).await {
                VerificationOutcome::Verified(verified) => verified.value,
                VerificationOutcome::Disagreement(failure) => {
                    return VerificationOutcome::Disagreement(failure);
                }
                VerificationOutcome::Unavailable(failure) => {
                    return VerificationOutcome::Unavailable(failure);
                }
                VerificationOutcome::Retryable(failure) => {
                    return VerificationOutcome::Retryable(failure);
                }
                VerificationOutcome::LocallyInvalid(failure) => {
                    return VerificationOutcome::LocallyInvalid(failure);
                }
            };

            if code.is_empty() || keccak256(&code) != expected_hash {
                return VerificationOutcome::Disagreement(
                    self.failure(VerificationRead::DeploymentIdentity),
                );
            }

            if let Some(proxy) = &contract.proxy {
                let slot = B256::from_str(&proxy.storage_slot).expect("validated proxy slot");
                let expected =
                    B256::from_str(&proxy.storage_value).expect("validated proxy storage value");
                match self.verify_storage(&address, &slot, block).await {
                    VerificationOutcome::Verified(verified) if verified.value == expected => {}
                    VerificationOutcome::Verified(_) => {
                        return VerificationOutcome::Disagreement(
                            self.failure(VerificationRead::DeploymentIdentity),
                        );
                    }
                    VerificationOutcome::Disagreement(failure) => {
                        return VerificationOutcome::Disagreement(failure);
                    }
                    VerificationOutcome::Unavailable(failure) => {
                        return VerificationOutcome::Unavailable(failure);
                    }
                    VerificationOutcome::Retryable(failure) => {
                        return VerificationOutcome::Retryable(failure);
                    }
                    VerificationOutcome::LocallyInvalid(failure) => {
                        return VerificationOutcome::LocallyInvalid(failure);
                    }
                }
                let target =
                    Address::from_str(&proxy.target_address).expect("validated proxy target");
                let target_hash =
                    B256::from_str(&proxy.target_code_hash).expect("validated proxy target hash");
                match self.verify_code(&target, block).await {
                    VerificationOutcome::Verified(verified)
                        if !verified.value.is_empty()
                            && keccak256(&verified.value) == target_hash => {}
                    VerificationOutcome::Verified(_) => {
                        return VerificationOutcome::Disagreement(
                            self.failure(VerificationRead::DeploymentIdentity),
                        );
                    }
                    VerificationOutcome::Disagreement(failure) => {
                        return VerificationOutcome::Disagreement(failure);
                    }
                    VerificationOutcome::Unavailable(failure) => {
                        return VerificationOutcome::Unavailable(failure);
                    }
                    VerificationOutcome::Retryable(failure) => {
                        return VerificationOutcome::Retryable(failure);
                    }
                    VerificationOutcome::LocallyInvalid(failure) => {
                        return VerificationOutcome::LocallyInvalid(failure);
                    }
                }
            }

            for probe in &contract.probes {
                let call_data =
                    Bytes::from_str(&probe.call_data).expect("validated manifest probe data");
                let expected = Bytes::from_str(&probe.expected_output)
                    .expect("validated manifest probe output");

                match self
                    .verify_call(None, &address, U256::ZERO, &call_data, block)
                    .await
                {
                    VerificationOutcome::Verified(verified) if verified.value == expected => {}
                    VerificationOutcome::Verified(_) => {
                        return VerificationOutcome::Disagreement(
                            self.failure(VerificationRead::DeploymentIdentity),
                        );
                    }
                    VerificationOutcome::Disagreement(failure) => {
                        return VerificationOutcome::Disagreement(failure);
                    }
                    VerificationOutcome::Unavailable(failure) => {
                        return VerificationOutcome::Unavailable(failure);
                    }
                    VerificationOutcome::Retryable(failure) => {
                        return VerificationOutcome::Retryable(failure);
                    }
                    VerificationOutcome::LocallyInvalid(failure) => {
                        return VerificationOutcome::LocallyInvalid(failure);
                    }
                }
            }
        }

        let normalized_value_digest = keccak256(
            serde_json::to_vec(manifest).expect("validated manifest serialization is infallible"),
        );
        VerificationOutcome::Verified(Verified {
            value: (),
            read: VerificationRead::DeploymentIdentity,
            provider_ids: self.provider_ids(),
            normalized_value_digest,
        })
    }

    pub(crate) async fn verify_storage(
        &self,
        address: &Address,
        slot: &B256,
        block: u64,
    ) -> VerificationOutcome<B256> {
        let (a, b, c) = tokio::join!(
            self.sources[0].storage_at(address, slot, block),
            self.sources[1].storage_at(address, slot, block),
            self.sources[2].storage_at(address, slot, block),
        );
        classify_exact(VerificationRead::ContractStorage, [a, b, c], &self.sources)
    }

    pub(crate) async fn verify_transaction_count(
        &self,
        address: &Address,
        block: u64,
    ) -> VerificationOutcome<u64> {
        let (a, b, c) = tokio::join!(
            self.sources[0].transaction_count_at(address, block),
            self.sources[1].transaction_count_at(address, block),
            self.sources[2].transaction_count_at(address, block),
        );
        classify_exact(VerificationRead::TransactionCount, [a, b, c], &self.sources)
    }

    pub(crate) async fn verify_pending_transaction_count(
        &self,
        address: &Address,
    ) -> VerificationOutcome<u64> {
        let (a, b, c) = tokio::join!(
            self.sources[0].transaction_count_pending(address),
            self.sources[1].transaction_count_pending(address),
            self.sources[2].transaction_count_pending(address),
        );
        classify_exact(
            VerificationRead::PendingTransactionCount,
            [a, b, c],
            &self.sources,
        )
    }

    pub(crate) async fn verify_reconciliation_pending_transaction_count(
        &self,
        address: &Address,
        owned_nonce: u64,
    ) -> VerificationOutcome<u64> {
        let (a, b, c) = tokio::join!(
            self.sources[0].transaction_count_pending(address),
            self.sources[1].transaction_count_pending(address),
            self.sources[2].transaction_count_pending(address),
        );
        let results = [a, b, c];
        let failure = || self.failure(VerificationRead::PendingTransactionCount);
        let values = results
            .iter()
            .filter_map(|result| result.as_ref().ok().copied())
            .collect::<Vec<_>>();
        let distinct = values.iter().copied().collect::<HashSet<_>>();
        if distinct.len() > 1 {
            let Some(next_nonce) = owned_nonce.checked_add(1) else {
                return VerificationOutcome::LocallyInvalid(failure());
            };
            return if distinct
                .iter()
                .all(|value| (owned_nonce..=next_nonce).contains(value))
            {
                VerificationOutcome::Retryable(failure())
            } else {
                VerificationOutcome::Disagreement(failure())
            };
        }

        if results.iter().any(is_permanent_capability_error) {
            return VerificationOutcome::LocallyInvalid(failure());
        }

        if results.iter().any(Result::is_err) {
            return VerificationOutcome::Unavailable(failure());
        }
        let value = values[0];
        VerificationOutcome::Verified(Verified {
            value,
            read: VerificationRead::PendingTransactionCount,
            provider_ids: self.provider_ids(),
            normalized_value_digest: normalized_digest(&value),
        })
    }

    pub(crate) async fn verify_balance(
        &self,
        address: &Address,
        block: u64,
    ) -> VerificationOutcome<U256> {
        let (a, b, c) = tokio::join!(
            self.sources[0].balance_at(address, block),
            self.sources[1].balance_at(address, block),
            self.sources[2].balance_at(address, block),
        );
        classify_exact(VerificationRead::Balance, [a, b, c], &self.sources)
    }

    pub(crate) async fn verify_call(
        &self,
        from: Option<&Address>,
        to: &Address,
        value: U256,
        data: &[u8],
        block: u64,
    ) -> VerificationOutcome<Bytes> {
        let (a, b, c) = tokio::join!(
            self.sources[0].call_at(from, to, value, data, block),
            self.sources[1].call_at(from, to, value, data, block),
            self.sources[2].call_at(from, to, value, data, block),
        );
        classify_exact(VerificationRead::ContractCall, [a, b, c], &self.sources)
    }

    pub(crate) async fn verify_decoded_call<T, F>(
        &self,
        from: Option<&Address>,
        to: &Address,
        value: U256,
        data: &[u8],
        block: u64,
        decode: F,
    ) -> VerificationOutcome<T>
    where
        T: Eq + NormalizedValue,
        F: Fn(&[u8]) -> anyhow::Result<T> + Copy,
    {
        let (a, b, c) = tokio::join!(
            self.sources[0].call_at(from, to, value, data, block),
            self.sources[1].call_at(from, to, value, data, block),
            self.sources[2].call_at(from, to, value, data, block),
        );
        classify_exact(
            VerificationRead::ContractCall,
            [
                a.and_then(|result| decode(&result)),
                b.and_then(|result| decode(&result)),
                c.and_then(|result| decode(&result)),
            ],
            &self.sources,
        )
    }

    pub(crate) async fn verify_decoded_simulation<T, F>(
        &self,
        from: &Address,
        to: &Address,
        value: U256,
        data: &[u8],
        block: u64,
        decode: F,
    ) -> VerificationOutcome<VerifiedSimulation<T>>
    where
        T: Eq + NormalizedValue,
        F: Fn(&[u8]) -> anyhow::Result<T> + Copy,
    {
        let (a, b, c) = tokio::join!(
            self.sources[0].call_result_at(Some(from), to, value, data, block),
            self.sources[1].call_result_at(Some(from), to, value, data, block),
            self.sources[2].call_result_at(Some(from), to, value, data, block),
        );
        classify_exact(
            VerificationRead::ContractCall,
            [
                decode_simulation(a, decode),
                decode_simulation(b, decode),
                decode_simulation(c, decode),
            ],
            &self.sources,
        )
    }

    pub(crate) async fn verify_quote_exact_input_single(
        &self,
        quote_contract: &Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee: alloy::primitives::aliases::U24,
        block: u64,
    ) -> VerificationOutcome<UniswapV3Quote> {
        let call = quote_exact_input_single_call(token_in, token_out, amount_in, fee);
        self.verify_quote(
            quote_contract,
            &call,
            block,
            decode_quote_exact_input_single,
        )
        .await
    }

    pub(crate) async fn verify_quote_exact_output_single(
        &self,
        quote_contract: &Address,
        token_in: Address,
        token_out: Address,
        amount_out: U256,
        fee: alloy::primitives::aliases::U24,
        block: u64,
    ) -> VerificationOutcome<UniswapV3Quote> {
        let call = quote_exact_output_single_call(token_in, token_out, amount_out, fee);
        self.verify_quote(
            quote_contract,
            &call,
            block,
            decode_quote_exact_output_single,
        )
        .await
    }

    async fn verify_quote(
        &self,
        quote_contract: &Address,
        call: &Bytes,
        block: u64,
        decode: fn(&[u8]) -> anyhow::Result<UniswapV3Quote>,
    ) -> VerificationOutcome<UniswapV3Quote> {
        let (a, b, c) = tokio::join!(
            self.sources[0].call_at(None, quote_contract, U256::ZERO, call, block),
            self.sources[1].call_at(None, quote_contract, U256::ZERO, call, block),
            self.sources[2].call_at(None, quote_contract, U256::ZERO, call, block),
        );
        classify_exact(
            VerificationRead::Quote,
            [
                a.and_then(|value| decode(&value)),
                b.and_then(|value| decode(&value)),
                c.and_then(|value| decode(&value)),
            ],
            &self.sources,
        )
    }

    pub(crate) async fn verify_gas_estimate(
        &self,
        from: &Address,
        to: &Address,
        value: U256,
        data: &[u8],
        block: u64,
    ) -> VerificationOutcome<u64> {
        let (a, b, c) = tokio::join!(
            self.sources[0].estimate_gas_at(from, to, value, data, block),
            self.sources[1].estimate_gas_at(from, to, value, data, block),
            self.sources[2].estimate_gas_at(from, to, value, data, block),
        );
        classify_all(
            VerificationRead::GasEstimate,
            [a, b, c],
            &self.sources,
            |values| *values.iter().max().unwrap(),
        )
    }

    pub(crate) async fn verify_priority_fee(&self) -> VerificationOutcome<u128> {
        let (a, b, c) = tokio::join!(
            self.sources[0].priority_fee(),
            self.sources[1].priority_fee(),
            self.sources[2].priority_fee(),
        );
        classify_all(
            VerificationRead::PriorityFee,
            [a, b, c],
            &self.sources,
            |mut values| {
                values.sort_unstable();
                values[1]
            },
        )
    }

    pub(crate) async fn verify_receipt(
        &self,
        tx_hash: &B256,
    ) -> VerificationOutcome<RpcTransactionReceipt> {
        let (a, b, c) = tokio::join!(
            self.sources[0].receipt(tx_hash),
            self.sources[1].receipt(tx_hash),
            self.sources[2].receipt(tx_hash),
        );
        classify_receipts([a, b, c], &self.sources)
    }

    pub(crate) async fn verify_receipt_absence(&self, tx_hash: &B256) -> VerificationOutcome<bool> {
        let (a, b, c) = tokio::join!(
            self.sources[0].receipt(tx_hash),
            self.sources[1].receipt(tx_hash),
            self.sources[2].receipt(tx_hash),
        );
        classify_receipt_absence([a, b, c], &self.sources)
    }

    pub(crate) async fn verify_transaction(
        &self,
        tx_hash: &B256,
    ) -> VerificationOutcome<RpcTransaction> {
        let (a, b, c) = tokio::join!(
            self.sources[0].transaction(tx_hash),
            self.sources[1].transaction(tx_hash),
            self.sources[2].transaction(tx_hash),
        );
        classify_propagating(VerificationRead::Transaction, [a, b, c], &self.sources)
    }

    pub(crate) async fn verify_replacement_block(
        &self,
        number: u64,
    ) -> VerificationOutcome<RpcBlock> {
        let (a, b, c) = tokio::join!(
            self.sources[0].block_with_transactions(number),
            self.sources[1].block_with_transactions(number),
            self.sources[2].block_with_transactions(number),
        );
        classify_exact(VerificationRead::ReplacementBlock, [a, b, c], &self.sources)
    }

    pub(crate) async fn verify_replacement_window(
        &self,
        parent: VerifiedBlockHeader,
        end_height: u64,
    ) -> VerificationOutcome<Vec<RpcBlock>> {
        if end_height < parent.number
            || end_height.saturating_sub(parent.number) > VERIFICATION_HEADER_WINDOW_MAX
        {
            return VerificationOutcome::LocallyInvalid(
                self.failure(VerificationRead::ReplacementBlock),
            );
        }

        let mut previous = parent;
        let mut blocks = Vec::with_capacity((end_height - previous.number) as usize);
        for number in previous.number + 1..=end_height {
            match self.verify_replacement_block(number).await {
                VerificationOutcome::Verified(verified)
                    if verified.value.parent_hash == previous.hash =>
                {
                    previous = VerifiedBlockHeader::from(verified.value.clone());
                    blocks.push(verified.value);
                }
                VerificationOutcome::Verified(_) => {
                    return VerificationOutcome::Disagreement(
                        self.failure(VerificationRead::ReplacementBlock),
                    );
                }
                VerificationOutcome::Disagreement(failure) => {
                    return VerificationOutcome::Disagreement(failure);
                }
                VerificationOutcome::Unavailable(failure) => {
                    return VerificationOutcome::Unavailable(failure);
                }
                VerificationOutcome::Retryable(failure) => {
                    return VerificationOutcome::Retryable(failure);
                }
                VerificationOutcome::LocallyInvalid(failure) => {
                    return VerificationOutcome::LocallyInvalid(failure);
                }
            }
        }

        VerificationOutcome::Verified(Verified {
            normalized_value_digest: normalized_digest(&blocks),
            value: blocks,
            read: VerificationRead::ReplacementBlock,
            provider_ids: self.provider_ids(),
        })
    }

    pub(crate) async fn verify_call_trace(
        &self,
        tx_hash: &B256,
    ) -> VerificationOutcome<VerifiedCallTrace> {
        let (a, b, c) = tokio::join!(
            self.sources[0].call_trace(tx_hash),
            self.sources[1].call_trace(tx_hash),
            self.sources[2].call_trace(tx_hash),
        );
        classify_exact(
            VerificationRead::CallTrace,
            [a.map(Into::into), b.map(Into::into), c.map(Into::into)],
            &self.sources,
        )
    }

    pub(crate) async fn verify_call_trace_capability(&self) -> VerificationOutcome<()> {
        let (a, b, c) = tokio::join!(
            self.sources[0].probe_call_trace(),
            self.sources[1].probe_call_trace(),
            self.sources[2].probe_call_trace(),
        );
        classify_exact(VerificationRead::CallTrace, [a, b, c], &self.sources)
    }

    fn provider_ids(&self) -> [String; VERIFICATION_SOURCE_COUNT] {
        source_provider_ids(&self.sources)
    }

    fn failure(&self, read: VerificationRead) -> VerificationFailure {
        VerificationFailure {
            read,
            provider_ids: self.provider_ids(),
        }
    }
}

fn validate_config(
    authoritative_url: &str,
    config: &BlockchainVerificationConfig,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        config.verifiers.len() == VERIFICATION_PROVIDER_COUNT,
        "`verification.verifiers` must contain exactly two providers"
    );
    validate_identity(&config.authoritative)?;
    for verifier in &config.verifiers {
        validate_identity(&verifier.identity)?;
    }

    let identities = [
        &config.authoritative,
        &config.verifiers[0].identity,
        &config.verifiers[1].identity,
    ];
    ensure_distinct(
        identities
            .iter()
            .map(|identity| identity.provider_id.as_str()),
        "provider IDs",
    )?;
    ensure_distinct(
        identities
            .iter()
            .map(|identity| identity.operator_id.as_str()),
        "operator IDs",
    )?;

    for left in 0..identities.len() {
        for right in left + 1..identities.len() {
            let left_domains = identities[left]
                .failure_domain_ids
                .iter()
                .collect::<HashSet<_>>();
            anyhow::ensure!(
                identities[right]
                    .failure_domain_ids
                    .iter()
                    .all(|domain| !left_domains.contains(domain)),
                "Provider failure domains must be pairwise disjoint"
            );
        }
    }

    let endpoints = [
        normalize_endpoint(authoritative_url)?,
        normalize_endpoint(config.verifiers[0].http_rpc_url.expose_secret())?,
        normalize_endpoint(config.verifiers[1].http_rpc_url.expose_secret())?,
    ];
    ensure_distinct(endpoints.iter().map(String::as_str), "provider endpoints")?;

    let anchor = &config.chain_anchor;
    anyhow::ensure!(anchor.chain_id != 0, "Chain anchor ID must be nonzero");
    anyhow::ensure!(
        !anchor.chain_name.trim().is_empty(),
        "Chain anchor name is required"
    );
    let checkpoint_hash = B256::from_str(&anchor.checkpoint_hash)
        .map_err(|_| anyhow::anyhow!("Chain checkpoint hash must contain 32 hexadecimal bytes"))?;
    anyhow::ensure!(
        checkpoint_hash != B256::ZERO,
        "Chain checkpoint hash must be nonzero"
    );
    anyhow::ensure!(
        anchor.max_head_skew_blocks != 0
            && anchor.max_head_age_secs != 0
            && anchor.max_future_drift_secs != 0,
        "Chain head skew, age, and future-drift limits must be nonzero"
    );
    anyhow::ensure!(
        !config.manifest_version.trim().is_empty(),
        "Deployment manifest version is required"
    );
    let manifest_digest = B256::from_str(&config.manifest_digest).map_err(|_| {
        anyhow::anyhow!("Deployment manifest digest must contain 32 hexadecimal bytes")
    })?;
    anyhow::ensure!(
        manifest_digest != B256::ZERO,
        "Deployment manifest digest must be nonzero"
    );
    let manifest = &config.deployment_manifest;
    anyhow::ensure!(
        manifest.version == config.manifest_version,
        "Deployment manifest version does not match its configured identity"
    );
    anyhow::ensure!(
        manifest.chain_id == anchor.chain_id && manifest.chain_name == anchor.chain_name,
        "Deployment manifest chain identity does not match the chain anchor"
    );
    let canonical_manifest = serde_json::to_vec(manifest)
        .map_err(|_| anyhow::anyhow!("Failed to serialize the deployment manifest"))?;
    anyhow::ensure!(
        keccak256(canonical_manifest) == manifest_digest,
        "Deployment manifest digest does not match its canonical content"
    );
    anyhow::ensure!(
        !manifest.contracts.is_empty() && !manifest.tokens.is_empty() && !manifest.pools.is_empty(),
        "Deployment manifest contracts, tokens, and pools are required"
    );
    let mut contract_addresses = HashSet::new();
    let mut roles = HashSet::new();

    for contract in &manifest.contracts {
        let address = Address::from_str(&contract.address)
            .map_err(|_| anyhow::anyhow!("Deployment manifest contains an invalid address"))?;
        anyhow::ensure!(
            contract_addresses.insert(address),
            "Deployment manifest contains a duplicate contract address"
        );
        roles.insert(contract.role);
        let code_hash = B256::from_str(&contract.runtime_code_hash)
            .map_err(|_| anyhow::anyhow!("Deployment manifest contains an invalid code hash"))?;
        anyhow::ensure!(
            code_hash != B256::ZERO,
            "Manifest code hashes must be nonzero"
        );
        anyhow::ensure!(
            matches!(contract.role, BlockchainContractRole::Implementation)
                || !contract.probes.is_empty(),
            "Every deployed contract role requires at least one identity probe"
        );

        if let Some(proxy) = &contract.proxy {
            anyhow::ensure!(
                matches!(
                    proxy.kind.as_str(),
                    "eip1967_implementation" | "zeppelinos_implementation"
                ),
                "Proxy kind is unsupported"
            );
            B256::from_str(&proxy.storage_slot)
                .map_err(|_| anyhow::anyhow!("Proxy storage slot is invalid"))?;
            let storage_value = B256::from_str(&proxy.storage_value)
                .map_err(|_| anyhow::anyhow!("Proxy storage value is invalid"))?;
            let target = Address::from_str(&proxy.target_address)
                .map_err(|_| anyhow::anyhow!("Proxy target address is invalid"))?;
            let target_hash = B256::from_str(&proxy.target_code_hash)
                .map_err(|_| anyhow::anyhow!("Proxy target code hash is invalid"))?;
            anyhow::ensure!(
                target != Address::ZERO
                    && target != address
                    && target_hash != B256::ZERO
                    && storage_value[..12].iter().all(|byte| *byte == 0)
                    && storage_value[12..] == *target.as_slice(),
                "Proxy target binding is invalid"
            );
        }

        for probe in &contract.probes {
            Bytes::from_str(&probe.call_data)
                .map_err(|_| anyhow::anyhow!("Manifest probe call data is invalid"))?;
            Bytes::from_str(&probe.expected_output)
                .map_err(|_| anyhow::anyhow!("Manifest probe output is invalid"))?;
        }
    }

    for contract in &manifest.contracts {
        let Some(proxy) = &contract.proxy else {
            continue;
        };
        let target =
            Address::from_str(&proxy.target_address).expect("validated proxy target address");
        let target_contract = manifest
            .contracts
            .iter()
            .find(|candidate| Address::from_str(&candidate.address).ok() == Some(target))
            .ok_or_else(|| {
                anyhow::anyhow!("Proxy target has no unique deployment manifest identity")
            })?;
        anyhow::ensure!(
            target_contract.role == BlockchainContractRole::Implementation
                && target_contract
                    .runtime_code_hash
                    .eq_ignore_ascii_case(&proxy.target_code_hash),
            "Proxy target role or code hash conflicts with its deployment identity"
        );
    }

    for required in [
        BlockchainContractRole::Router,
        BlockchainContractRole::Factory,
        BlockchainContractRole::WrappedNative,
        BlockchainContractRole::Quote,
        BlockchainContractRole::Token,
        BlockchainContractRole::Pool,
    ] {
        anyhow::ensure!(
            roles.contains(&required),
            "Deployment manifest is missing a required role"
        );
    }

    for token in &manifest.tokens {
        let address = Address::from_str(&token.address)
            .map_err(|_| anyhow::anyhow!("Token manifest address is invalid"))?;
        anyhow::ensure!(
            contract_addresses.contains(&address),
            "Token manifest address has no contract identity"
        );
        anyhow::ensure!(
            !token.name.trim().is_empty()
                && !token.symbol.trim().is_empty()
                && matches!(token.asset_role.as_str(), "base" | "quote" | "both"),
            "Token manifest identity or asset role is invalid"
        );
    }

    for pool in &manifest.pools {
        for address in [
            &pool.address,
            &pool.token0,
            &pool.token1,
            &pool.factory,
            &pool.quote_contract,
        ] {
            let address = Address::from_str(address)
                .map_err(|_| anyhow::anyhow!("Pool manifest address is invalid"))?;
            anyhow::ensure!(
                contract_addresses.contains(&address),
                "Pool manifest references an unpinned contract"
            );
        }
        anyhow::ensure!(
            pool.fee != 0 && pool.fee <= 1_000_000,
            "Pool fee is invalid"
        );
    }
    let mut call_edges = HashSet::new();
    for edge in &manifest.call_edges {
        anyhow::ensure!(
            matches!(
                edge.purpose.as_str(),
                "wrap" | "approve" | "swap_sell" | "swap_buy"
            ) && matches!(
                edge.call_type.as_str(),
                "call" | "staticcall" | "delegatecall" | "callcode"
            ),
            "Deployment manifest call edge is invalid"
        );
        let caller = Address::from_str(&edge.caller)
            .map_err(|_| anyhow::anyhow!("Call edge address is invalid"))?;
        let target = Address::from_str(&edge.target)
            .map_err(|_| anyhow::anyhow!("Call edge address is invalid"))?;
        for address in [caller, target] {
            anyhow::ensure!(
                contract_addresses.contains(&address),
                "Call edge references an unpinned contract"
            );
        }
        anyhow::ensure!(
            call_edges.insert((
                edge.purpose.as_str(),
                caller,
                target,
                edge.call_type.as_str()
            )),
            "Deployment manifest contains a duplicate call edge"
        );
    }

    for purpose in ["swap_sell", "swap_buy"] {
        anyhow::ensure!(
            manifest
                .call_edges
                .iter()
                .any(|edge| edge.purpose == purpose),
            "Deployment manifest is missing a swap call graph"
        );
    }
    Ok(())
}

fn validate_identity(identity: &BlockchainProviderIdentity) -> anyhow::Result<()> {
    anyhow::ensure!(
        !identity.provider_id.trim().is_empty(),
        "Provider ID is required"
    );
    anyhow::ensure!(
        !identity.operator_id.trim().is_empty(),
        "Provider operator ID is required"
    );
    anyhow::ensure!(
        !identity.failure_domain_ids.is_empty()
            && identity
                .failure_domain_ids
                .iter()
                .all(|domain| !domain.trim().is_empty()),
        "Provider failure domains must contain nonempty opaque IDs"
    );
    ensure_distinct(
        identity.failure_domain_ids.iter().map(String::as_str),
        "failure-domain IDs within one provider",
    )
}

fn ensure_distinct<'a>(
    values: impl IntoIterator<Item = &'a str>,
    description: &str,
) -> anyhow::Result<()> {
    let values = values.into_iter().collect::<Vec<_>>();
    let unique = values.iter().copied().collect::<HashSet<_>>();
    anyhow::ensure!(values.len() == unique.len(), "Duplicate {description}");
    Ok(())
}

fn normalize_endpoint(endpoint: &str) -> anyhow::Result<String> {
    let url = validate_execution_endpoint(endpoint, "Provider")?;
    Ok(url.to_string())
}

fn decode_simulation<T>(
    result: anyhow::Result<RpcCallResult>,
    decode: impl Fn(&[u8]) -> anyhow::Result<T>,
) -> anyhow::Result<VerifiedSimulation<T>> {
    match result? {
        RpcCallResult::Success(bytes) => decode(&bytes).map(VerifiedSimulation::Succeeded),
        RpcCallResult::Reverted => Ok(VerifiedSimulation::Denied),
    }
}

fn classify_exact<T: Eq + NormalizedValue>(
    read: VerificationRead,
    results: [anyhow::Result<T>; VERIFICATION_SOURCE_COUNT],
    sources: &[VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
) -> VerificationOutcome<T> {
    let provider_ids = source_provider_ids(sources);
    let failure = || VerificationFailure {
        read,
        provider_ids: provider_ids.clone(),
    };
    let values = results.iter().filter_map(|result| result.as_ref().ok());
    let mut distinct = Vec::<&T>::new();
    for value in values {
        if !distinct.contains(&value) {
            distinct.push(value);
        }
    }

    if distinct.len() > 1 {
        return VerificationOutcome::Disagreement(failure());
    }

    if results.iter().any(is_permanent_capability_error) {
        return VerificationOutcome::LocallyInvalid(failure());
    }

    if results.iter().any(Result::is_err) {
        return VerificationOutcome::Unavailable(failure());
    }

    let normalized_value_digest = normalized_digest(distinct[0]);
    VerificationOutcome::Verified(Verified {
        value: results.into_iter().next().unwrap().unwrap(),
        read,
        provider_ids,
        normalized_value_digest,
    })
}

fn classify_all<T, U: NormalizedValue>(
    read: VerificationRead,
    results: [anyhow::Result<T>; VERIFICATION_SOURCE_COUNT],
    sources: &[VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
    select: impl FnOnce([T; VERIFICATION_SOURCE_COUNT]) -> U,
) -> VerificationOutcome<U> {
    let provider_ids = source_provider_ids(sources);
    let failure = || VerificationFailure {
        read,
        provider_ids: provider_ids.clone(),
    };

    if results.iter().any(is_permanent_capability_error) {
        return VerificationOutcome::LocallyInvalid(failure());
    }

    if results.iter().any(Result::is_err) {
        return VerificationOutcome::Unavailable(failure());
    }
    let value = select(results.map(Result::unwrap));
    VerificationOutcome::Verified(Verified {
        normalized_value_digest: normalized_digest(&value),
        value,
        read,
        provider_ids,
    })
}

fn classify_propagating<T: Clone + Eq + NormalizedValue>(
    read: VerificationRead,
    results: [anyhow::Result<Option<T>>; VERIFICATION_SOURCE_COUNT],
    sources: &[VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
) -> VerificationOutcome<T> {
    let provider_ids = source_provider_ids(sources);
    let failure = || VerificationFailure {
        read,
        provider_ids: provider_ids.clone(),
    };
    let present = results
        .iter()
        .filter_map(|result| result.as_ref().ok().and_then(Option::as_ref))
        .collect::<Vec<_>>();

    if present.windows(2).any(|values| values[0] != values[1]) {
        return VerificationOutcome::Disagreement(failure());
    }

    if results.iter().any(is_permanent_capability_error) {
        return VerificationOutcome::LocallyInvalid(failure());
    }

    if results.iter().any(Result::is_err) {
        return VerificationOutcome::Unavailable(failure());
    }

    if present.len() != VERIFICATION_SOURCE_COUNT {
        return VerificationOutcome::Retryable(failure());
    }
    VerificationOutcome::Verified(Verified {
        normalized_value_digest: normalized_digest(present[0]),
        value: present[0].clone(),
        read,
        provider_ids,
    })
}

fn classify_receipts(
    results: [anyhow::Result<Option<RpcTransactionReceipt>>; VERIFICATION_SOURCE_COUNT],
    sources: &[VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
) -> VerificationOutcome<RpcTransactionReceipt> {
    let read = VerificationRead::Receipt;
    let provider_ids = source_provider_ids(sources);
    let failure = || VerificationFailure {
        read,
        provider_ids: provider_ids.clone(),
    };
    let normalized = normalize_receipts(&results);
    let present = normalized
        .iter()
        .filter_map(|result| result.as_ref().ok().and_then(Option::as_ref))
        .collect::<Vec<_>>();

    if present.windows(2).any(|values| values[0] != values[1]) {
        return VerificationOutcome::Disagreement(failure());
    }

    if results.iter().any(is_permanent_capability_error) {
        return VerificationOutcome::LocallyInvalid(failure());
    }

    if results.iter().any(Result::is_err) || normalized.iter().any(Result::is_err) {
        return VerificationOutcome::Unavailable(failure());
    }

    if present.len() != VERIFICATION_SOURCE_COUNT {
        return VerificationOutcome::Retryable(failure());
    }
    let normalized_value_digest = normalized_digest(present[0]);
    let value = results.into_iter().next().unwrap().unwrap().unwrap();
    VerificationOutcome::Verified(Verified {
        value,
        read,
        provider_ids,
        normalized_value_digest,
    })
}

fn classify_receipt_absence(
    results: [anyhow::Result<Option<RpcTransactionReceipt>>; VERIFICATION_SOURCE_COUNT],
    sources: &[VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
) -> VerificationOutcome<bool> {
    let read = VerificationRead::Receipt;
    let provider_ids = source_provider_ids(sources);
    let failure = || VerificationFailure {
        read,
        provider_ids: provider_ids.clone(),
    };
    let normalized = normalize_receipts(&results);
    let present = normalized
        .iter()
        .filter_map(|result| result.as_ref().ok().and_then(Option::as_ref))
        .collect::<Vec<_>>();

    if present.windows(2).any(|values| values[0] != values[1]) {
        return VerificationOutcome::Disagreement(failure());
    }

    if results.iter().any(is_permanent_capability_error) {
        return VerificationOutcome::LocallyInvalid(failure());
    }

    if results.iter().any(Result::is_err) || normalized.iter().any(Result::is_err) {
        return VerificationOutcome::Unavailable(failure());
    }

    if present.is_empty() {
        return VerificationOutcome::Verified(Verified {
            value: true,
            read,
            provider_ids,
            normalized_value_digest: normalized_digest(&true),
        });
    }

    if present.len() == VERIFICATION_SOURCE_COUNT {
        return VerificationOutcome::Verified(Verified {
            value: false,
            read,
            provider_ids,
            normalized_value_digest: normalized_digest(&false),
        });
    }
    VerificationOutcome::Retryable(failure())
}

#[expect(
    clippy::result_large_err,
    reason = "The typed verification failure must preserve its complete three-source evidence"
)]
fn collect_values<T>(
    read: VerificationRead,
    results: [anyhow::Result<T>; VERIFICATION_SOURCE_COUNT],
    sources: &[VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
) -> Result<[T; VERIFICATION_SOURCE_COUNT], VerificationOutcome<VerifiedBlockHeader>> {
    let failure = VerificationFailure {
        read,
        provider_ids: source_provider_ids(sources),
    };

    if results.iter().any(is_permanent_capability_error) {
        return Err(VerificationOutcome::LocallyInvalid(failure));
    }

    if results.iter().any(Result::is_err) {
        return Err(VerificationOutcome::Unavailable(failure));
    }
    Ok(results.map(Result::unwrap))
}

fn source_provider_ids(
    sources: &[VerificationRpcClient; VERIFICATION_SOURCE_COUNT],
) -> [String; VERIFICATION_SOURCE_COUNT] {
    sources
        .each_ref()
        .map(|source| source.identity.provider_id.clone())
}

fn normalize_receipts(
    results: &[anyhow::Result<Option<RpcTransactionReceipt>>; VERIFICATION_SOURCE_COUNT],
) -> [anyhow::Result<Option<VerifiedReceipt>>; VERIFICATION_SOURCE_COUNT] {
    results.each_ref().map(|result| {
        result
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.to_string()))
            .and_then(|receipt| receipt.clone().map(TryInto::try_into).transpose())
    })
}

fn is_permanent_capability_error<T>(result: &anyhow::Result<T>) -> bool {
    result.as_ref().is_err_and(|e| {
        let message = e.to_string();
        message.contains("RPC error -32601")
            || message.contains("RPC error -32602")
            || message.contains("redirect rejected")
    })
}

pub(crate) trait NormalizedValue {
    fn write_normalized(&self, output: &mut Vec<u8>);
}

fn normalized_digest(value: &impl NormalizedValue) -> B256 {
    let mut normalized = Vec::new();
    value.write_normalized(&mut normalized);
    keccak256(normalized)
}

macro_rules! impl_normalized_integer {
    ($type:ty, $tag:expr) => {
        impl NormalizedValue for $type {
            fn write_normalized(&self, output: &mut Vec<u8>) {
                output.push($tag);
                output.extend_from_slice(&self.to_be_bytes());
            }
        }
    };
}

impl_normalized_integer!(u8, 1);
impl_normalized_integer!(u32, 2);
impl_normalized_integer!(u64, 3);
impl_normalized_integer!(u128, 4);

impl NormalizedValue for bool {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.extend_from_slice(&[5, u8::from(*self)]);
    }
}

impl NormalizedValue for () {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(23);
    }
}

impl NormalizedValue for Address {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(6);
        output.extend_from_slice(self.as_slice());
    }
}

impl NormalizedValue for B256 {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(7);
        output.extend_from_slice(self.as_slice());
    }
}

impl NormalizedValue for U256 {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(8);
        output.extend_from_slice(&self.to_be_bytes::<32>());
    }
}

impl NormalizedValue for Bytes {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(9);
        output.extend_from_slice(&(self.len() as u64).to_be_bytes());
        output.extend_from_slice(self);
    }
}

impl<T: NormalizedValue> NormalizedValue for Option<T> {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        match self {
            Some(value) => {
                output.push(10);
                value.write_normalized(output);
            }
            None => output.push(11),
        }
    }
}

impl<T: NormalizedValue> NormalizedValue for Vec<T> {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(12);
        output.extend_from_slice(&(self.len() as u64).to_be_bytes());
        for value in self {
            value.write_normalized(output);
        }
    }
}

impl NormalizedValue for VerifiedBlockHeader {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(13);
        self.number.write_normalized(output);
        self.hash.write_normalized(output);
        self.parent_hash.write_normalized(output);
        self.timestamp.write_normalized(output);
        self.base_fee_per_gas.write_normalized(output);
    }
}

impl NormalizedValue for UniswapV3Quote {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(14);
        self.amount.write_normalized(output);
        output.extend_from_slice(&self.sqrt_price_x96_after.to_be_bytes::<20>());
        self.initialized_ticks_crossed.write_normalized(output);
        self.gas_estimate.write_normalized(output);
    }
}

impl<T: NormalizedValue> NormalizedValue for VerifiedSimulation<T> {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        match self {
            Self::Succeeded(value) => {
                output.push(15);
                value.write_normalized(output);
            }
            Self::Denied => output.push(16),
        }
    }
}

impl NormalizedValue for RpcTransaction {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(17);
        self.hash.write_normalized(output);
        self.from.write_normalized(output);
        self.nonce.write_normalized(output);
        self.chain_id.write_normalized(output);
        self.transaction_type.write_normalized(output);
        self.to.write_normalized(output);
        self.input.write_normalized(output);
        self.value.write_normalized(output);
        self.gas.write_normalized(output);
        self.max_fee_per_gas.write_normalized(output);
        self.max_priority_fee_per_gas.write_normalized(output);
    }
}

impl NormalizedValue for RpcBlock {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(23);
        self.number.write_normalized(output);
        self.hash.write_normalized(output);
        self.parent_hash.write_normalized(output);
        self.timestamp.write_normalized(output);
        self.base_fee_per_gas.write_normalized(output);
        self.transactions.write_normalized(output);
    }
}

impl NormalizedValue for RpcCallType {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        let value = match self {
            Self::Call => 0,
            Self::Callcode => 1,
            Self::Delegatecall => 2,
            Self::Staticcall => 3,
            Self::Create => 4,
            Self::Create2 => 5,
            Self::Selfdestruct => 6,
        };
        output.extend_from_slice(&[18, value]);
    }
}

impl NormalizedValue for [u8; 4] {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(19);
        output.extend_from_slice(self);
    }
}

impl NormalizedValue for VerifiedCallTrace {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(20);
        self.call_type.write_normalized(output);
        self.from.write_normalized(output);
        self.to.write_normalized(output);
        self.value.write_normalized(output);
        self.input_selector.write_normalized(output);
        self.input_digest.write_normalized(output);
        self.success.write_normalized(output);
        self.calls.write_normalized(output);
    }
}

impl NormalizedValue for VerifiedReceiptLog {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(21);
        self.removed.write_normalized(output);
        self.log_index.write_normalized(output);
        self.transaction_index.write_normalized(output);
        self.transaction_hash.write_normalized(output);
        self.block_hash.write_normalized(output);
        self.block_number.write_normalized(output);
        self.address.write_normalized(output);
        self.data.write_normalized(output);
        self.topics.write_normalized(output);
    }
}

impl NormalizedValue for VerifiedReceipt {
    fn write_normalized(&self, output: &mut Vec<u8>) {
        output.push(22);
        self.transaction_hash.write_normalized(output);
        self.block_hash.write_normalized(output);
        self.block_number.write_normalized(output);
        self.gas_used.write_normalized(output);
        self.effective_gas_price.write_normalized(output);
        self.transaction_index.write_normalized(output);
        self.status.write_normalized(output);
        self.logs.write_normalized(output);
    }
}

fn parse_optional_hash(value: Option<&str>) -> anyhow::Result<Option<B256>> {
    value
        .map(|value| B256::from_str(value).map_err(|_| anyhow::anyhow!("Invalid receipt log hash")))
        .transpose()
}

fn parse_optional_quantity(value: Option<&str>) -> anyhow::Result<Option<u64>> {
    value
        .map(|value| {
            let value = value.strip_prefix("0x").unwrap_or(value);
            u64::from_str_radix(value, 16)
                .map_err(|_| anyhow::anyhow!("Invalid receipt log quantity"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use alloy::{
        primitives::{
            U256, address,
            aliases::{U24, U160},
            b256,
        },
        sol_types::SolValue,
    };
    use rstest::rstest;

    use super::*;
    use crate::rpc::http::tests::mock::{MockRpcState, start_mock_rpc_server};

    const CHAIN_ID_ARBITRUM: &str =
        include_str!("../../test_data/execution/rpc_eth_chain_id_arbitrum.json");
    const CHAIN_ID_ETHEREUM: &str =
        include_str!("../../test_data/execution/rpc_eth_chain_id_ethereum.json");
    const RECEIPT_NULL: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_null.json");
    const RECEIPT_SUCCESS: &str =
        include_str!("../../test_data/execution/rpc_eth_get_transaction_receipt_success.json");
    const QUOTE_SELECTOR: &str = "0xc6a5026a";
    const CALL_SELECTOR: &str = "0x12345678";
    const CALL_EMPTY: &str = r#"{"jsonrpc":"2.0","id":1,"result":"0x"}"#;
    const CALL_REVERTED: &str =
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":3,"message":"execution reverted"}}"#;
    const TRACE_UNKNOWN_TRANSACTION: &str =
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"transaction not found"}}"#;

    fn identity(provider: &str, operator: &str, domain: &str) -> BlockchainProviderIdentity {
        BlockchainProviderIdentity {
            provider_id: provider.to_string(),
            operator_id: operator.to_string(),
            failure_domain_ids: vec![domain.to_string()],
        }
    }

    fn quote_response(amount: u64) -> String {
        let encoded = (
            U256::from(amount),
            U160::from(1u128 << 96),
            0u32,
            U256::from(50_000u64),
        )
            .abi_encode();
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": nautilus_core::hex::encode_prefixed(encoded),
        })
        .to_string()
    }

    fn config() -> BlockchainVerificationConfig {
        let contract = |address: &str, role| crate::config::BlockchainContractManifest {
            address: address.to_string(),
            role,
            runtime_code_hash: b256!(
                "3333333333333333333333333333333333333333333333333333333333333333"
            )
            .to_string(),
            proxy: None,
            probes: vec![crate::config::BlockchainContractProbe {
                call_data: "0x12345678".to_string(),
                expected_output: "0x".to_string(),
            }],
        };
        let deployment_manifest = crate::config::BlockchainDeploymentManifest {
            version: "arbitrum-v1".to_string(),
            chain_id: 42_161,
            chain_name: "Arbitrum One".to_string(),
            contracts: vec![
                contract(
                    "0x0000000000000000000000000000000000000001",
                    BlockchainContractRole::Router,
                ),
                contract(
                    "0x0000000000000000000000000000000000000002",
                    BlockchainContractRole::Factory,
                ),
                contract(
                    "0x0000000000000000000000000000000000000003",
                    BlockchainContractRole::WrappedNative,
                ),
                contract(
                    "0x0000000000000000000000000000000000000004",
                    BlockchainContractRole::Quote,
                ),
                contract(
                    "0x0000000000000000000000000000000000000005",
                    BlockchainContractRole::Token,
                ),
                contract(
                    "0x0000000000000000000000000000000000000006",
                    BlockchainContractRole::Pool,
                ),
            ],
            tokens: vec![crate::config::BlockchainTokenManifest {
                address: "0x0000000000000000000000000000000000000005".to_string(),
                name: "Token".to_string(),
                symbol: "TKN".to_string(),
                decimals: 18,
                asset_role: "both".to_string(),
            }],
            pools: vec![crate::config::BlockchainPoolManifest {
                address: "0x0000000000000000000000000000000000000006".to_string(),
                token0: "0x0000000000000000000000000000000000000005".to_string(),
                token1: "0x0000000000000000000000000000000000000003".to_string(),
                fee: 500,
                factory: "0x0000000000000000000000000000000000000002".to_string(),
                quote_contract: "0x0000000000000000000000000000000000000004".to_string(),
            }],
            call_edges: ["swap_sell", "swap_buy"]
                .into_iter()
                .map(|purpose| crate::config::BlockchainCallEdgeManifest {
                    purpose: purpose.to_string(),
                    caller: "0x0000000000000000000000000000000000000001".to_string(),
                    target: "0x0000000000000000000000000000000000000006".to_string(),
                    call_type: "call".to_string(),
                })
                .collect(),
        };
        let manifest_digest = keccak256(serde_json::to_vec(&deployment_manifest).unwrap());
        BlockchainVerificationConfig {
            authoritative: identity("authoritative", "operator-a", "domain-a"),
            verifiers: vec![
                BlockchainVerificationProviderConfig {
                    identity: identity("verifier-a", "operator-b", "domain-b"),
                    http_rpc_url: "https://verifier-a.example.com".into(),
                },
                BlockchainVerificationProviderConfig {
                    identity: identity("verifier-b", "operator-c", "domain-c"),
                    http_rpc_url: "https://verifier-b.example.com".into(),
                },
            ],
            chain_anchor: BlockchainChainAnchorConfig {
                chain_id: 42_161,
                chain_name: "Arbitrum One".to_string(),
                checkpoint_height: 1,
                checkpoint_hash: b256!(
                    "1111111111111111111111111111111111111111111111111111111111111111"
                )
                .to_string(),
                checkpoint_timestamp: 1_700_000_000,
                max_head_skew_blocks: 3,
                max_head_age_secs: 60,
                max_future_drift_secs: 5,
            },
            manifest_version: "arbitrum-v1".to_string(),
            manifest_digest: manifest_digest.to_string(),
            deployment_manifest,
        }
    }

    async fn coordinator(
        authoritative: MockRpcState,
        verifier_a: MockRpcState,
        verifier_b: MockRpcState,
    ) -> VerificationCoordinator {
        let authoritative_addr = start_mock_rpc_server(authoritative).await;
        let verifier_a_addr = start_mock_rpc_server(verifier_a).await;
        let verifier_b_addr = start_mock_rpc_server(verifier_b).await;
        let authoritative_url = format!("http://{authoritative_addr}");
        let mut config = config();
        config.verifiers[0].http_rpc_url = format!("http://{verifier_a_addr}").into();
        config.verifiers[1].http_rpc_url = format!("http://{verifier_b_addr}").into();
        let authoritative_rpc = Arc::new(BlockchainHttpRpcClient::new(
            authoritative_url.clone(),
            None,
            None,
        ));
        VerificationCoordinator::new(authoritative_rpc, &authoritative_url, &config, None).unwrap()
    }

    #[rstest]
    fn normalized_digest_uses_canonical_binary_encoding() {
        let expected = keccak256([3, 0, 0, 0, 0, 0, 0, 0, 42]);

        assert_eq!(normalized_digest(&42u64), expected);
        assert_ne!(normalized_digest(&42u64), normalized_digest(&43u64));
    }

    #[rstest]
    fn topology_accepts_three_independent_sources() {
        validate_config("https://authoritative.example.com", &config()).unwrap();
    }

    #[rstest]
    fn topology_rejects_remote_cleartext_authoritative_endpoint() {
        let error = validate_config("http://authoritative.example.com", &config()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Provider endpoint must use HTTPS unless its host is a canonical loopback IP literal"
        );
    }

    #[rstest]
    fn topology_rejects_remote_cleartext_verifier_endpoint() {
        let mut config = config();
        config.verifiers[0].http_rpc_url = "http://verifier-a.example.com".into();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Provider endpoint must use HTTPS unless its host is a canonical loopback IP literal"
        );
    }

    #[rstest]
    #[case(1)]
    #[case(3)]
    fn topology_requires_exactly_two_verifiers(#[case] count: usize) {
        let mut config = config();
        let verifier = config.verifiers[0].clone();
        config.verifiers.resize(count, verifier);

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "`verification.verifiers` must contain exactly two providers"
        );
    }

    #[rstest]
    fn topology_rejects_duplicate_provider() {
        let mut config = config();
        config.verifiers[0].identity.provider_id = "authoritative".to_string();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(error.to_string(), "Duplicate provider IDs");
    }

    #[rstest]
    fn topology_rejects_duplicate_operator() {
        let mut config = config();
        config.verifiers[0].identity.operator_id = "operator-a".to_string();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(error.to_string(), "Duplicate operator IDs");
    }

    #[rstest]
    fn topology_rejects_shared_failure_domain() {
        let mut config = config();
        config.verifiers[1].identity.failure_domain_ids = vec!["domain-a".to_string()];

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Provider failure domains must be pairwise disjoint"
        );
    }

    #[rstest]
    fn manifest_rejects_missing_role_probe() {
        let mut config = config();
        config.deployment_manifest.contracts[0].probes.clear();
        config.manifest_digest =
            keccak256(serde_json::to_vec(&config.deployment_manifest).unwrap()).to_string();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert!(error.to_string().contains("identity probe"), "was: {error}");
    }

    #[rstest]
    fn manifest_rejects_unknown_proxy_kind() {
        let mut config = config();
        config.deployment_manifest.contracts[0].proxy =
            Some(crate::config::BlockchainProxyManifest {
                kind: "custom".to_string(),
                storage_slot: B256::from([1; 32]).to_string(),
                storage_value: B256::from([2; 32]).to_string(),
                target_address: "0x0000000000000000000000000000000000000007".to_string(),
                target_code_hash: B256::from([3; 32]).to_string(),
            });
        config.manifest_digest =
            keccak256(serde_json::to_vec(&config.deployment_manifest).unwrap()).to_string();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert!(error.to_string().contains("unsupported"), "was: {error}");
    }

    #[rstest]
    fn manifest_rejects_eip1967_beacon_binding() {
        let mut config = config();
        config.deployment_manifest.contracts[0].proxy =
            Some(crate::config::BlockchainProxyManifest {
                kind: "eip1967_beacon".to_string(),
                storage_slot: B256::from([1; 32]).to_string(),
                storage_value: B256::from([2; 32]).to_string(),
                target_address: "0x0000000000000000000000000000000000000007".to_string(),
                target_code_hash: B256::from([3; 32]).to_string(),
            });
        config.manifest_digest =
            keccak256(serde_json::to_vec(&config.deployment_manifest).unwrap()).to_string();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(error.to_string(), "Proxy kind is unsupported");
    }

    #[rstest]
    fn manifest_schema_rejects_removed_precompile_field() {
        let mut manifest = serde_json::to_value(config().deployment_manifest).unwrap();
        manifest["permitted_precompiles"] =
            serde_json::json!(["0x0000000000000000000000000000000000000001"]);

        let error = serde_json::from_value::<BlockchainDeploymentManifest>(manifest).unwrap_err();

        assert!(error.to_string().contains("unknown field"), "was: {error}");
        assert!(
            error.to_string().contains("permitted_precompiles"),
            "was: {error}"
        );
    }

    #[rstest]
    fn manifest_schema_rejects_removed_beacon_role() {
        let mut manifest = serde_json::to_value(config().deployment_manifest).unwrap();
        manifest["contracts"][0]["role"] = serde_json::json!("beacon");

        let error = serde_json::from_value::<BlockchainDeploymentManifest>(manifest).unwrap_err();

        assert!(
            error.to_string().contains("unknown variant"),
            "was: {error}"
        );
        assert!(error.to_string().contains("beacon"), "was: {error}");
    }

    #[rstest]
    fn manifest_accepts_zeppelinos_implementation_binding() {
        let mut config = config();
        let target_address = "0x0000000000000000000000000000000000000007";
        let target_code_hash = B256::from([4; 32]).to_string();
        config
            .deployment_manifest
            .contracts
            .push(crate::config::BlockchainContractManifest {
                address: target_address.to_string(),
                role: BlockchainContractRole::Implementation,
                runtime_code_hash: target_code_hash.clone(),
                proxy: None,
                probes: Vec::new(),
            });
        config.deployment_manifest.contracts[4].proxy =
            Some(crate::config::BlockchainProxyManifest {
                kind: "zeppelinos_implementation".to_string(),
                storage_slot: B256::from([1; 32]).to_string(),
                storage_value: "0x0000000000000000000000000000000000000000000000000000000000000007"
                    .to_string(),
                target_address: target_address.to_string(),
                target_code_hash,
            });
        config.manifest_digest =
            keccak256(serde_json::to_vec(&config.deployment_manifest).unwrap()).to_string();

        validate_config("https://authoritative.example.com", &config).unwrap();
    }

    #[tokio::test]
    async fn quote_requires_exact_three_source_agreement() {
        let quote_a = quote_response(100);
        let quote_b = quote_response(101);
        let coordinator = coordinator(
            MockRpcState::default().with_call_response(QUOTE_SELECTOR, &quote_a),
            MockRpcState::default().with_call_response(QUOTE_SELECTOR, &quote_a),
            MockRpcState::default().with_call_response(QUOTE_SELECTOR, &quote_b),
        )
        .await;

        let outcome = coordinator
            .verify_quote_exact_input_single(
                &address!("0000000000000000000000000000000000000004"),
                address!("0000000000000000000000000000000000000003"),
                address!("0000000000000000000000000000000000000005"),
                U256::from(1u64),
                U24::try_from(500u32).unwrap(),
                1,
            )
            .await;

        assert!(matches!(outcome, VerificationOutcome::Disagreement(_)));
    }

    #[tokio::test]
    async fn malformed_quote_is_unavailable() {
        let malformed = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x01",
        })
        .to_string();
        let coordinator = coordinator(
            MockRpcState::default().with_call_response(QUOTE_SELECTOR, &malformed),
            MockRpcState::default().with_call_response(QUOTE_SELECTOR, &malformed),
            MockRpcState::default().with_call_response(QUOTE_SELECTOR, &malformed),
        )
        .await;

        let outcome = coordinator
            .verify_quote_exact_input_single(
                &address!("0000000000000000000000000000000000000004"),
                address!("0000000000000000000000000000000000000003"),
                address!("0000000000000000000000000000000000000005"),
                U256::from(1u64),
                U24::try_from(500u32).unwrap(),
                1,
            )
            .await;

        assert!(matches!(outcome, VerificationOutcome::Unavailable(_)));
    }

    #[tokio::test]
    async fn three_simulation_reverts_are_verified_denial() {
        let coordinator = coordinator(
            MockRpcState::default().with_call_response(CALL_SELECTOR, CALL_REVERTED),
            MockRpcState::default().with_call_response(CALL_SELECTOR, CALL_REVERTED),
            MockRpcState::default().with_call_response(CALL_SELECTOR, CALL_REVERTED),
        )
        .await;

        let outcome = coordinator
            .verify_decoded_simulation(
                &address!("0000000000000000000000000000000000000001"),
                &address!("0000000000000000000000000000000000000002"),
                U256::ZERO,
                &[0x12, 0x34, 0x56, 0x78],
                1,
                |result| Ok(result.is_empty()),
            )
            .await;

        assert!(matches!(
            outcome,
            VerificationOutcome::Verified(Verified {
                value: VerifiedSimulation::Denied,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn mixed_simulation_success_and_revert_disagrees() {
        let coordinator = coordinator(
            MockRpcState::default().with_call_response(CALL_SELECTOR, CALL_EMPTY),
            MockRpcState::default().with_call_response(CALL_SELECTOR, CALL_REVERTED),
            MockRpcState::default().with_call_response(CALL_SELECTOR, CALL_REVERTED),
        )
        .await;

        let outcome = coordinator
            .verify_decoded_simulation(
                &address!("0000000000000000000000000000000000000001"),
                &address!("0000000000000000000000000000000000000002"),
                U256::ZERO,
                &[0x12, 0x34, 0x56, 0x78],
                1,
                |result| Ok(result.is_empty()),
            )
            .await;

        assert!(matches!(outcome, VerificationOutcome::Disagreement(_)));
    }

    #[tokio::test]
    async fn trace_probe_accepts_recognized_method_error() {
        let coordinator = coordinator(
            MockRpcState::default()
                .with_response("debug_traceTransaction", TRACE_UNKNOWN_TRANSACTION),
            MockRpcState::default()
                .with_response("debug_traceTransaction", TRACE_UNKNOWN_TRANSACTION),
            MockRpcState::default()
                .with_response("debug_traceTransaction", TRACE_UNKNOWN_TRANSACTION),
        )
        .await;

        let outcome = coordinator.verify_call_trace_capability().await;

        assert!(matches!(
            outcome,
            VerificationOutcome::Verified(Verified { value: (), .. })
        ));
    }

    #[tokio::test]
    async fn trace_probe_rejects_missing_method() {
        let coordinator = coordinator(
            MockRpcState::default(),
            MockRpcState::default(),
            MockRpcState::default(),
        )
        .await;

        let outcome = coordinator.verify_call_trace_capability().await;

        assert!(matches!(outcome, VerificationOutcome::LocallyInvalid(_)));
    }

    #[tokio::test]
    async fn missing_quote_method_is_locally_invalid() {
        let coordinator = coordinator(
            MockRpcState::default(),
            MockRpcState::default(),
            MockRpcState::default(),
        )
        .await;

        let outcome = coordinator
            .verify_quote_exact_input_single(
                &address!("0000000000000000000000000000000000000004"),
                address!("0000000000000000000000000000000000000003"),
                address!("0000000000000000000000000000000000000005"),
                U256::from(1u64),
                U24::try_from(500u32).unwrap(),
                1,
            )
            .await;

        assert!(matches!(outcome, VerificationOutcome::LocallyInvalid(_)));
    }

    #[rstest]
    fn topology_rejects_normalized_duplicate_endpoint() {
        let mut config = config();
        config.verifiers[0].http_rpc_url = "https://AUTHORITATIVE.example.com/".into();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(error.to_string(), "Duplicate provider endpoints");
    }

    #[rstest]
    fn topology_rejects_endpoint_fragment() {
        let mut config = config();
        config.verifiers[0].http_rpc_url = "https://verifier-a.example.com/#alias".into();

        let error = validate_config("https://authoritative.example.com", &config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Provider endpoint fragments are unsupported"
        );
    }

    #[tokio::test]
    async fn conflicting_valid_values_outrank_an_unavailable_source() {
        let coordinator = coordinator(
            MockRpcState::default().with_response("eth_chainId", CHAIN_ID_ARBITRUM),
            MockRpcState::default().with_response("eth_chainId", CHAIN_ID_ETHEREUM),
            MockRpcState::default(),
        )
        .await;

        let outcome = coordinator.verify_chain_id().await;

        assert!(matches!(outcome, VerificationOutcome::Disagreement(_)));
    }

    #[tokio::test]
    async fn missing_method_is_locally_invalid() {
        let coordinator = coordinator(
            MockRpcState::default(),
            MockRpcState::default(),
            MockRpcState::default(),
        )
        .await;

        let outcome = coordinator.verify_chain_id().await;

        assert!(matches!(outcome, VerificationOutcome::LocallyInvalid(_)));
    }

    #[tokio::test]
    async fn priority_fee_selects_median_of_three_valid_values() {
        let response =
            |value: u64| format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x{value:x}\"}}");
        let low = response(10);
        let median = response(20);
        let high = response(1_000);
        let coordinator = coordinator(
            MockRpcState::default().with_response("eth_maxPriorityFeePerGas", &low),
            MockRpcState::default().with_response("eth_maxPriorityFeePerGas", &high),
            MockRpcState::default().with_response("eth_maxPriorityFeePerGas", &median),
        )
        .await;

        let outcome = coordinator.verify_priority_fee().await;

        assert!(matches!(
            outcome,
            VerificationOutcome::Verified(Verified { value: 20, .. })
        ));
    }

    #[tokio::test]
    async fn reconciliation_pending_nonce_propagation_is_retryable() {
        let response =
            |value: u64| format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x{value:x}\"}}");
        let current = response(7);
        let next = response(8);
        let coordinator = coordinator(
            MockRpcState::default().with_response("eth_getTransactionCount", &current),
            MockRpcState::default().with_response("eth_getTransactionCount", &next),
            MockRpcState::default().with_response("eth_getTransactionCount", &current),
        )
        .await;

        let outcome = coordinator
            .verify_reconciliation_pending_transaction_count(
                &address!("0000000000000000000000000000000000000001"),
                7,
            )
            .await;

        assert!(matches!(outcome, VerificationOutcome::Retryable(_)));
    }

    #[tokio::test]
    async fn reconciliation_pending_nonce_outside_owned_range_disagrees() {
        let response =
            |value: u64| format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x{value:x}\"}}");
        let current = response(7);
        let outside = response(9);
        let coordinator = coordinator(
            MockRpcState::default().with_response("eth_getTransactionCount", &current),
            MockRpcState::default().with_response("eth_getTransactionCount", &outside),
            MockRpcState::default().with_response("eth_getTransactionCount", &current),
        )
        .await;

        let outcome = coordinator
            .verify_reconciliation_pending_transaction_count(
                &address!("0000000000000000000000000000000000000001"),
                7,
            )
            .await;

        assert!(matches!(outcome, VerificationOutcome::Disagreement(_)));
    }

    #[tokio::test]
    async fn partial_receipt_propagation_is_retryable() {
        let coordinator = coordinator(
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS),
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
        )
        .await;
        let tx_hash = b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a");

        let outcome = coordinator.verify_receipt(&tx_hash).await;

        assert!(matches!(outcome, VerificationOutcome::Retryable(_)));
    }

    #[tokio::test]
    async fn receipt_absence_requires_all_three_sources() {
        let coordinator = coordinator(
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_SUCCESS),
        )
        .await;
        let tx_hash = b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a");

        let outcome = coordinator.verify_receipt_absence(&tx_hash).await;

        assert!(matches!(outcome, VerificationOutcome::Retryable(_)));
    }

    #[tokio::test]
    async fn unanimous_receipt_absence_is_verified() {
        let coordinator = coordinator(
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
            MockRpcState::default().with_response("eth_getTransactionReceipt", RECEIPT_NULL),
        )
        .await;
        let tx_hash = b256!("9da4b71be3336357259f56bda5cfbd3803c211ce09b510c43e6fb2af84088c6a");

        let outcome = coordinator.verify_receipt_absence(&tx_hash).await;

        assert!(matches!(
            outcome,
            VerificationOutcome::Verified(Verified { value: true, .. })
        ));
    }
}
