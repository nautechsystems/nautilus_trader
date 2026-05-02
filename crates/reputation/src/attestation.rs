//! Attestation types for on-chain reputation.

use nautilus_core::UUID4;
use serde::{Deserialize, Serialize};

/// Outcome of a trade for attestation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeOutcome {
    /// Realized PnL in quote currency.
    pub realized_pnl: f64,
    /// Maximum adverse excursion.
    pub max_adverse_excursion: f64,
    /// Slippage in basis points.
    pub slippage_bps: f64,
    /// Whether the trade hit the intended target.
    pub target_hit: bool,
}

/// An attestation to be submitted on-chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attestation {
    /// Agent identifier.
    pub agent_id: UUID4,
    /// Hash of the decision (intent + context).
    pub decision_hash: [u8; 32],
    /// Trade outcome.
    pub outcome: TradeOutcome,
    /// Timestamp (unix nanos).
    pub timestamp_ns: u64,
    /// Stake amount (in wei or lamports).
    pub stake_amount: u128,
}

impl Attestation {
    /// Compute SHA3-256 hash of the attestation for on-chain submission.
    pub fn hash(&self) -> [u8; 32] {
        use sha3::{Digest, Sha3_256};
        let mut hasher = Sha3_256::new();
        hasher.update(self.agent_id.to_string().as_bytes());
        hasher.update(&self.decision_hash);
        hasher.update(&self.realized_pnl_bytes());
        hasher.update(self.timestamp_ns.to_le_bytes());
        hasher.finalize().into()
    }

    fn realized_pnl_bytes(&self) -> [u8; 8] {
        self.outcome.realized_pnl.to_le_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attestation_hash() {
        let att = Attestation {
            agent_id: UUID4::new(),
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

        let hash = att.hash();
        assert_ne!(hash, [0u8; 32]);
    }
}
