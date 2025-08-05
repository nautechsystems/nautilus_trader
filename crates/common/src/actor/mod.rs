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

//! Actor system for event-driven message processing.
//!
//! This module provides the actor framework used throughout NautilusTrader for handling
//! data processing, event management, and asynchronous message handling. Actors are
//! lightweight components that process messages in isolation.

#![allow(unsafe_code)]

use std::{any::Any, cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_model::identifiers::{ActorId, TraderId};
use ustr::Ustr;

use crate::{
    cache::Cache,
    clock::Clock,
    enums::{ActorState, ActorTrigger},
};

pub mod data_actor;
#[cfg(feature = "indicators")]
pub(crate) mod indicators;
pub mod registry;

#[cfg(test)]
mod tests;

// Re-exports
pub use data_actor::{DataActor, DataActorCore};

pub trait Actor: Any + Debug {
    /// The unique identifier for the actor.
    fn id(&self) -> Ustr;

    /// Handles the `msg`.
    fn handle(&mut self, msg: &dyn Any);

    /// Returns a reference to `self` as `Any`, for downcasting support.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to `self` as `Any`, for downcasting support.
    ///
    /// Default implementation simply coerces `&mut Self` to `&mut dyn Any`.
    ///
    /// # Note
    ///
    /// This method is not object-safe and thus only available on sized `Self`.
    fn as_any_mut(&mut self) -> &mut dyn Any
    where
        Self: Sized,
    {
        self
    }

    // Component lifecycle methods merged from Component trait

    /// Returns the unique identifier for this actor.
    ///
    /// # Deprecated
    ///
    /// This method is deprecated. Use `id()` directly instead of converting to `ActorId`.
    /// This method exists for compatibility but direct `id()` usage is preferred.
    fn actor_id(&self) -> ActorId;

    /// Returns the current state of the actor.
    fn state(&self) -> ActorState;

    /// Transition the actor with the state trigger.
    ///
    /// # Errors
    ///
    /// Returns an error if the `trigger` is an invalid transition from the current state.
    fn transition_state(&mut self, trigger: ActorTrigger) -> anyhow::Result<()>;

    /// Returns whether the actor is ready.
    fn is_ready(&self) -> bool {
        self.state() == ActorState::Ready
    }

    /// Returns whether the actor is *not* running.
    fn not_running(&self) -> bool {
        !self.is_running()
    }

    /// Returns whether the actor is running.
    fn is_running(&self) -> bool {
        self.state() == ActorState::Running
    }

    /// Returns whether the actor is stopped.
    fn is_stopped(&self) -> bool {
        self.state() == ActorState::Stopped
    }

    /// Returns whether the actor has been degraded.
    fn is_degraded(&self) -> bool {
        self.state() == ActorState::Degraded
    }

    /// Returns whether the actor has been faulted.
    fn is_faulted(&self) -> bool {
        self.state() == ActorState::Faulted
    }

    /// Returns whether the actor has been disposed.
    fn is_disposed(&self) -> bool {
        self.state() == ActorState::Disposed
    }

    /// Registers the component with a system.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to register.
    fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()>;

    /// Initializes the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if the initialization state transition fails.
    fn initialize(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Initialize)
    }

    /// Starts the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor fails to start.
    fn start(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Start)?; // -> Starting

        if let Err(e) = self.on_start() {
            log::error!("{e}");
            anyhow::bail!("Failed to start actor: {e}");
        }

        self.transition_state(ActorTrigger::StartCompleted)?;

        Ok(())
    }

    /// Stops the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor fails to stop.
    fn stop(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Stop)?; // -> Stopping

        if let Err(e) = self.on_stop() {
            log::error!("{e}");
            anyhow::bail!("Failed to stop actor: {e}");
        }

        self.transition_state(ActorTrigger::StopCompleted)?;

        Ok(())
    }

    /// Resumes the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor fails to resume.
    fn resume(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Resume)?; // -> Resuming

        if let Err(e) = self.on_resume() {
            log::error!("{e}");
            anyhow::bail!("Failed to resume actor: {e}");
        }

        self.transition_state(ActorTrigger::ResumeCompleted)?;

        Ok(())
    }

    /// Degrades the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor fails to degrade.
    fn degrade(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Degrade)?; // -> Degrading

        if let Err(e) = self.on_degrade() {
            log::error!("{e}");
            anyhow::bail!("Failed to degrade actor: {e}");
        }

        self.transition_state(ActorTrigger::DegradeCompleted)?;

        Ok(())
    }

    /// Faults the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor fails to fault.
    fn fault(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Fault)?; // -> Faulting

        if let Err(e) = self.on_fault() {
            log::error!("{e}");
            anyhow::bail!("Failed to fault actor: {e}");
        }

        self.transition_state(ActorTrigger::FaultCompleted)?;

        Ok(())
    }

    /// Resets the actor to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor fails to reset.
    fn reset(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Reset)?; // -> Resetting

        if let Err(e) = self.on_reset() {
            log::error!("{e}");
            anyhow::bail!("Failed to reset actor: {e}");
        }

        self.transition_state(ActorTrigger::ResetCompleted)?;

        Ok(())
    }

    /// Disposes of the actor, releasing any resources.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor fails to dispose.
    fn dispose(&mut self) -> anyhow::Result<()> {
        self.transition_state(ActorTrigger::Dispose)?; // -> Disposing

        if let Err(e) = self.on_dispose() {
            log::error!("{e}");
            anyhow::bail!("Failed to dispose actor: {e}");
        }

        self.transition_state(ActorTrigger::DisposeCompleted)?;

        Ok(())
    }

    /// Actions to be performed on start.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the actor fails.
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_start` handler was called when not overridden, \
            it's expected that any actions required when stopping the component \
            occur here, such as unsubscribing from data",
        );
        Ok(())
    }

    /// Actions to be performed on stop.
    ///
    /// # Errors
    ///
    /// Returns an error if stopping the actor fails.
    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_stop` handler was called when not overridden, \
            it's expected that any actions required when stopping the component \
            occur here, such as unsubscribing from data",
        );
        Ok(())
    }

    /// Actions to be performed on resume.
    ///
    /// # Errors
    ///
    /// Returns an error if resuming the actor fails.
    fn on_resume(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_resume` handler was called when not overridden, \
            it's expected that any actions required when resuming the component \
            following a stop occur here"
        );
        Ok(())
    }

    /// Actions to be performed on reset.
    ///
    /// # Errors
    ///
    /// Returns an error if resetting the actor fails.
    fn on_reset(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_reset` handler was called when not overridden, \
            it's expected that any actions required when resetting the component \
            occur here, such as resetting indicators and other state"
        );
        Ok(())
    }

    /// Actions to be performed on dispose.
    ///
    /// # Errors
    ///
    /// Returns an error if disposing the actor fails.
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on degrade.
    ///
    /// # Errors
    ///
    /// Returns an error if degrading the actor fails.
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on fault.
    ///
    /// # Errors
    ///
    /// Returns an error if faulting the actor fails.
    fn on_fault(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
