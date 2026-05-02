//! Agent trait definition.

use async_trait::async_trait;
use nautilus_state_encoder::ContextWindow;

use crate::intent::AgentIntent;

/// Feedback from execution back to the agent.
#[derive(Clone, Debug)]
pub struct AgentFeedback {
    /// The intent that was executed.
    pub intent_id: nautilus_core::UUID4,
    /// Whether execution succeeded.
    pub success: bool,
    /// Actual fill price (average).
    pub fill_price: Option<f64>,
    /// Actual fill quantity.
    pub fill_quantity: Option<f64>,
    /// Slippage in basis points.
    pub slippage_bps: Option<f64>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// The unified interface for all agents in the swarm.
#[async_trait]
pub trait Agent: Send + Sync {
    /// Unique agent identifier.
    fn id(&self) -> &str;

    /// Perceive the current state and produce a trading intent.
    async fn perceive(&mut self, ctx: &ContextWindow) -> AgentIntent;

    /// Receive execution feedback and update internal state.
    async fn on_feedback(&mut self, feedback: &AgentFeedback);

    /// Agent's current confidence level [0.0, 1.0].
    fn confidence(&self) -> f64 {
        0.5
    }
}
