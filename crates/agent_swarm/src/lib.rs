//! Agent swarm for KuaaMU Quant Engine.
//!
//! Defines the Agent trait, Intent types, IntentCompiler, and SwarmCoordinator
//! for multi-agent trading with AI-native decision making.

pub mod agent;
pub mod compiler;
pub mod intent;
pub mod swarm;

// Re-export core types
pub use agent::{Agent, AgentFeedback};
pub use compiler::IntentCompiler;
pub use intent::{AgentIntent, ExecutionDirective, ExecutionStyle, IntentType};
pub use swarm::{ConsensusStrategy, SwarmCoordinator};
