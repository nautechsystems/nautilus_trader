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

//! Defines a generic `Finite-State Machine` (FSM).
//!
//! The FSM operates with a state-transition table of tuples and enums. The
//! intended use case is to ensure correct state transitions, as well as holding a
//! deterministic state value.
//!
//! # References
//!
//! <https://en.wikipedia.org/wiki/Finite-state_machine>

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::hash::Hash;

/// Error representing an invalid trigger for the current state.
#[derive(Debug)]
pub struct InvalidStateTrigger {
    /// The current state as a string.
    pub current_state: String,
    /// The trigger as a string.
    pub trigger: String,
}

impl fmt::Display for InvalidStateTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid state transition: {} -> {}", self.current_state, self.trigger)
    }
}

impl Error for InvalidStateTrigger {}

/// Provides a generic finite state machine.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use nautilus_core::fsm::FiniteStateMachine;
///
/// // Define states and triggers as enums
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum State {
///     Initialized,
///     Running,
///     Stopped,
/// }
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum Trigger {
///     Start,
///     Stop,
/// }
///
/// // Create a state transition table
/// let mut state_transition_table = HashMap::new();
/// state_transition_table.insert((State::Initialized, Trigger::Start), State::Running);
/// state_transition_table.insert((State::Running, Trigger::Stop), State::Stopped);
///
/// // Create the FSM
/// let mut fsm = FiniteStateMachine::new(
///     State::Initialized,
///     state_transition_table,
///     |t| format!("{:?}", t),
///     |s| format!("{:?}", s),
/// );
///
/// // Trigger state transitions
/// fsm.trigger(Trigger::Start);
/// assert_eq!(fsm.state_string(), "Running");
///
/// fsm.trigger(Trigger::Stop);
/// assert_eq!(fsm.state_string(), "Stopped");
/// ```
///
/// Invalid transitions will cause a panic:
///
/// ```should_panic
/// use std::collections::HashMap;
/// use nautilus_core::fsm::FiniteStateMachine;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum State {
///     Initialized,
///     Running,
/// }
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum Trigger {
///     Start,
///     Stop,
/// }
///
/// let mut state_transition_table = HashMap::new();
/// state_transition_table.insert((State::Initialized, Trigger::Start), State::Running);
///
/// let mut fsm = FiniteStateMachine::new(
///     State::Initialized,
///     state_transition_table,
///     |t| format!("{:?}", t),
///     |s| format!("{:?}", s),
/// );
///
/// // This will panic because there's no transition defined for Stop from Initialized
/// fsm.trigger(Trigger::Stop);
/// ```
pub struct FiniteStateMachine<S, T>
where
    S: Copy + Eq + Hash,
    T: Copy + Eq + Hash,
{
    state: S,
    state_transition_table: HashMap<(S, T), S>,
    state_parser: fn(S) -> String,
    trigger_parser: fn(T) -> String,
}

impl<S, T> fmt::Debug for FiniteStateMachine<S, T>
where
    S: Copy + Eq + Hash + fmt::Debug,
    T: Copy + Eq + Hash + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FiniteStateMachine")
            .field("state", &self.state)
            .field("state_transition_table", &self.state_transition_table)
            .finish_non_exhaustive()
    }
}

impl<S, T> FiniteStateMachine<S, T>
where
    S: Copy + Eq + Hash,
    T: Copy + Eq + Hash,
{
    /// Creates a new finite state machine.
    ///
    /// # Arguments
    ///
    /// * `state_transition_table` - The state-transition table for the FSM consisting of a tuple of
    ///   starting state and trigger as keys, and resulting states as values.
    /// * `initial_state` - The initial state for the FSM.
    /// * `trigger_parser` - The trigger parser needed to convert triggers into strings.
    /// * `state_parser` - The state parser needed to convert states into strings.
    ///
    /// # Returns
    ///
    /// A new `FiniteStateMachine` instance.
    ///
    /// # Panics
    ///
    /// Panics if `state_transition_table` is empty.
    pub fn new(
        initial_state: S,
        state_transition_table: HashMap<(S, T), S>,
        trigger_parser: fn(T) -> String,
        state_parser: fn(S) -> String,
    ) -> Self {
        if state_transition_table.is_empty() {
            panic!("state_transition_table cannot be empty");
        }

        Self {
            state: initial_state,
            state_transition_table,
            trigger_parser,
            state_parser,
        }
    }

    /// Returns the current state.
    ///
    /// # Returns
    ///
    /// The current state.
    pub fn state(&self) -> S {
        self.state
    }

    /// Returns the current state as a string.
    ///
    /// # Returns
    ///
    /// The current state as a string.
    pub fn state_string(&self) -> String {
        (self.state_parser)(self.state)
    }

    /// Process the FSM with the given trigger. The trigger must be valid for
    /// the FSMs current state.
    ///
    /// # Arguments
    ///
    /// * `trigger` - The trigger to combine with the current state providing the key for
    ///   the transition table lookup.
    ///
    /// # Panics
    ///
    /// Panics with an `InvalidStateTrigger` message if the state and `trigger` combination
    /// is not found in the transition table.
    pub fn trigger(&mut self, trigger: T) {
        if let Some(&next_state) = self.state_transition_table.get(&(self.state, trigger)) {
            self.state = next_state;
        } else {
            panic!("Invalid state transition: {} -> {}",
                self.state_string(),
                (self.trigger_parser)(trigger)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestState {
        Initialized,
        Running,
        Stopped,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestTrigger {
        Start,
        Stop,
    }

    #[fixture]
    fn state_transition_table() -> HashMap<(TestState, TestTrigger), TestState> {
        let mut table = HashMap::new();
        table.insert((TestState::Initialized, TestTrigger::Start), TestState::Running);
        table.insert((TestState::Running, TestTrigger::Stop), TestState::Stopped);
        table
    }

    #[fixture]
    fn fsm(state_transition_table: HashMap<(TestState, TestTrigger), TestState>) -> FiniteStateMachine<TestState, TestTrigger> {
        FiniteStateMachine::new(
            TestState::Initialized,
            state_transition_table,
            |t| format!("{:?}", t),
            |s| format!("{:?}", s),
        )
    }

    #[rstest]
    fn test_initial_state(fsm: FiniteStateMachine<TestState, TestTrigger>) {
        assert_eq!(fsm.state(), TestState::Initialized);
        assert_eq!(fsm.state_string(), "Initialized");
    }

    #[rstest]
    #[case(TestTrigger::Start, TestState::Running, "Running")]
    fn test_valid_single_transition(
        #[case] trigger: TestTrigger,
        #[case] expected_state: TestState,
        #[case] expected_state_string: &str,
        mut fsm: FiniteStateMachine<TestState, TestTrigger>,
    ) {
        fsm.trigger(trigger);
        assert_eq!(fsm.state(), expected_state);
        assert_eq!(fsm.state_string(), expected_state_string);
    }

    #[rstest]
    fn test_valid_multiple_transitions(mut fsm: FiniteStateMachine<TestState, TestTrigger>) {
        // Initialized -> Running
        fsm.trigger(TestTrigger::Start);
        assert_eq!(fsm.state(), TestState::Running);
        assert_eq!(fsm.state_string(), "Running");

        // Running -> Stopped
        fsm.trigger(TestTrigger::Stop);
        assert_eq!(fsm.state(), TestState::Stopped);
        assert_eq!(fsm.state_string(), "Stopped");
    }

    #[rstest]
    #[should_panic(expected = "Invalid state transition: Initialized -> Stop")]
    fn test_invalid_transition(mut fsm: FiniteStateMachine<TestState, TestTrigger>) {
        // This should panic
        fsm.trigger(TestTrigger::Stop);
    }

    #[rstest]
    #[should_panic(expected = "state_transition_table cannot be empty")]
    fn test_empty_transition_table() {
        let state_transition_table = HashMap::<(TestState, TestTrigger), TestState>::new();
        FiniteStateMachine::new(
            TestState::Initialized,
            state_transition_table,
            |t: TestTrigger| format!("{:?}", t),
            |s: TestState| format!("{:?}", s),
        );
    }
}
