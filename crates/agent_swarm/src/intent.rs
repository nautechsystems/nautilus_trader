//! AgentIntent and ExecutionDirective types.

use nautilus_core::UUID4;
use nautilus_model::identifiers::InstrumentId;
use std::time::Duration;

/// Types of trading intents an agent can produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntentType {
    DeltaHedge,
    GammaScalp,
    TrendFollow,
    MeanReversion,
    LiquidationCapture,
    Hold,
}

/// Position target specification.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionTarget {
    /// Target position size (absolute).
    pub size: f64,
    /// Target delta exposure (for options).
    pub delta: Option<f64>,
}

/// Risk budget consumed by this intent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiskBudget {
    /// Maximum acceptable loss for this intent (in quote currency).
    pub max_loss: f64,
    /// Maximum position size after execution.
    pub max_position: f64,
    /// Maximum drawdown contribution (basis points).
    pub max_drawdown_bps: f64,
}

/// Soft constraints for execution.
#[derive(Clone, Debug, PartialEq)]
pub struct Constraint {
    pub name: String,
    pub value: ConstraintValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstraintValue {
    Duration(Duration),
    Price(f64),
    SlippageBps(f64),
    Volume(f64),
}

/// An agent's trading intent — natural language + structured parameters.
#[derive(Clone, Debug)]
pub struct AgentIntent {
    pub id: UUID4,
    pub agent_id: String,
    pub intent_type: IntentType,
    /// Natural language description (for audit trail).
    pub description: String,
    pub target_instrument: InstrumentId,
    pub target_position: Option<PositionTarget>,
    pub risk_budget: RiskBudget,
    pub constraints: Vec<Constraint>,
    /// Agent's confidence in this intent [0.0, 1.0].
    pub confidence: f64,
    /// Time horizon for execution.
    pub time_horizon: Duration,
}

impl AgentIntent {
    pub fn hold(agent_id: &str, instrument: InstrumentId) -> Self {
        Self {
            id: UUID4::new(),
            agent_id: agent_id.to_string(),
            intent_type: IntentType::Hold,
            description: "Hold current position".to_string(),
            target_instrument: instrument,
            target_position: None,
            risk_budget: RiskBudget {
                max_loss: 0.0,
                max_position: 0.0,
                max_drawdown_bps: 0.0,
            },
            constraints: vec![],
            confidence: 1.0,
            time_horizon: Duration::from_secs(300),
        }
    }
}

/// The execution style for an order.
#[derive(Clone, Debug, PartialEq)]
pub enum ExecutionStyle {
    Twap {
        slices: u32,
        interval: Duration,
    },
    Vwap {
        volume_profile: Vec<f64>,
    },
    Ioc,
    Fok,
    Limit {
        price: f64,
        post_only: bool,
    },
    TrailingStop {
        offset_bps: f64,
    },
}

/// Engine-compiled execution directive.
#[derive(Clone, Debug)]
pub struct ExecutionDirective {
    pub intent_id: UUID4,
    pub orders: Vec<OrderSpecification>,
    pub execution_style: ExecutionStyle,
    pub time_horizon: Duration,
    pub max_slippage_bps: f64,
}

/// Specification for a single order within a directive.
#[derive(Clone, Debug)]
pub struct OrderSpecification {
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub quantity: f64,
    pub price: Option<f64>,
    pub time_in_force: TimeInForce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeInForce {
    Gtc,  // Good Till Cancel
    Ioc,  // Immediate Or Cancel
    Fok,  // Fill Or Kill
    Gtd,  // Good Till Date
}
