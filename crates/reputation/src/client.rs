//! On-chain reputation client (stub for ERC-8004 integration).

use nautilus_core::UUID4;

use crate::attestation::Attestation;
use crate::autonomy::AutonomyLevel;

/// Client for interacting with on-chain reputation registry.
///
/// This is a stub — real implementation will use ethers-rs (EVM)
/// or solana-client (Solana) depending on target chain.
pub struct ReputationClient {
    pub agent_id: UUID4,
    pub chain: Chain,
    /// Cached reputation score.
    cached_score: f64,
}

#[derive(Clone, Copy, Debug)]
pub enum Chain {
    Ethereum,
    Solana,
}

impl ReputationClient {
    pub fn new(agent_id: UUID4, chain: Chain) -> Self {
        Self {
            agent_id,
            chain,
            cached_score: 0.0,
        }
    }

    /// Submit an attestation on-chain.
    pub async fn attest(&self, _attestation: &Attestation) -> Result<TxHash, String> {
        // Stub: in production, this would submit to the chain
        tracing::info!("Attestation submitted for agent {}", self.agent_id);
        Ok(TxHash([0u8; 32]))
    }

    /// Query current reputation score.
    pub async fn query_score(&mut self) -> f64 {
        // Stub: in production, this would query the chain
        self.cached_score
    }

    /// Get autonomy level based on current score.
    pub async fn autonomy_level(&mut self) -> AutonomyLevel {
        let score = self.query_score().await;
        AutonomyLevel::from_score(score)
    }

    /// Update cached score (for testing).
    pub fn set_cached_score(&mut self, score: f64) {
        self.cached_score = score;
    }
}

/// Transaction hash wrapper.
#[derive(Clone, Copy, Debug)]
pub struct TxHash(pub [u8; 32]);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::TradeOutcome;

    #[tokio::test]
    async fn test_client_attest() {
        let client = ReputationClient::new(UUID4::new(), Chain::Ethereum);
        let att = Attestation {
            agent_id: client.agent_id,
            decision_hash: [0u8; 32],
            outcome: TradeOutcome {
                realized_pnl: 100.0,
                max_adverse_excursion: 50.0,
                slippage_bps: 10.0,
                target_hit: true,
            },
            timestamp_ns: 1_000_000_000,
            stake_amount: 1_000_000,
        };

        let result = client.attest(&att).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_autonomy_from_cached_score() {
        let mut client = ReputationClient::new(UUID4::new(), Chain::Solana);
        client.set_cached_score(85.0);

        let level = client.autonomy_level().await;
        assert_eq!(level, AutonomyLevel::High);
    }
}
