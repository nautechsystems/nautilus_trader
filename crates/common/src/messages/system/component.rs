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

use std::{
    any::Any,
    fmt::{Debug, Display},
};

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::TraderId;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::enums::ComponentState;

/// Represents an event which includes information on the state of a component.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
pub struct ComponentStateChanged {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The component ID associated with the event.
    pub component_id: Ustr,
    /// The component type.
    pub component_type: Ustr,
    /// The component state.
    pub state: ComponentState,
    /// The component configuration.
    pub config: IndexMap<String, String>,
    /// The event ID.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl ComponentStateChanged {
    /// Creates a new [`ComponentStateChanged`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        component_id: Ustr,
        component_type: Ustr,
        state: ComponentState,
        config: IndexMap<String, String>,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            component_id,
            component_type,
            state,
            config,
            event_id,
            ts_event,
            ts_init,
        }
    }

    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Display for ComponentStateChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, component_id={}, component_type={}, state={}, event_id={})",
            stringify!(ComponentStateChanged),
            self.trader_id,
            self.component_id,
            self.component_type,
            self.state,
            self.event_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn component_state_changed() -> ComponentStateChanged {
        let mut config = IndexMap::new();
        config.insert("timeout_secs".to_string(), "30".to_string());

        ComponentStateChanged::new(
            TraderId::from("TESTER-001"),
            Ustr::from("DataEngine"),
            Ustr::from("DataEngine"),
            ComponentState::Running,
            config,
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            UnixNanos::from(3),
            UnixNanos::from(5),
        )
    }

    #[rstest]
    fn test_new_assigns_every_field() {
        let event = component_state_changed();

        assert_eq!(event.trader_id, TraderId::from("TESTER-001"));
        assert_eq!(event.component_id, Ustr::from("DataEngine"));
        assert_eq!(event.component_type, Ustr::from("DataEngine"));
        assert_eq!(event.state, ComponentState::Running);
        assert_eq!(event.config.get("timeout_secs").unwrap(), "30");
        assert_eq!(
            event.event_id,
            UUID4::from("00000000-0000-4000-8000-000000000001")
        );
        assert_eq!(event.ts_event, UnixNanos::from(3));
        assert_eq!(event.ts_init, UnixNanos::from(5));
    }

    #[rstest]
    fn test_display_reports_identity_state_and_event_id() {
        assert_eq!(
            component_state_changed().to_string(),
            "ComponentStateChanged(trader_id=TESTER-001, component_id=DataEngine, \
             component_type=DataEngine, state=RUNNING, \
             event_id=00000000-0000-4000-8000-000000000001)"
        );
    }

    #[rstest]
    fn test_as_any_downcasts_to_self() {
        let event = component_state_changed();

        assert_eq!(
            event.as_any().downcast_ref::<ComponentStateChanged>(),
            Some(&event)
        );
    }

    #[rstest]
    fn test_serde_round_trips() {
        let event = component_state_changed();

        let json = serde_json::to_string(&event).unwrap();

        assert_eq!(
            serde_json::from_str::<ComponentStateChanged>(&json).unwrap(),
            event
        );
    }
}
