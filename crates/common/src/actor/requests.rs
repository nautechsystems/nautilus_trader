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

use std::{cell::Cell, collections::VecDeque, rc::Rc};

use ahash::{AHashMap, AHashSet};
use nautilus_core::UUID4;

use crate::{messages::data::DataResponseEnvelope, msgbus::TypedHandler};

#[derive(Debug, Default)]
pub(super) struct RequestLifecycle {
    requests: AHashMap<UUID4, RequestState>,
    groups: AHashMap<UUID4, RequestGroup>,
    parents: AHashMap<UUID4, Vec<UUID4>>,
    ready: VecDeque<UUID4>,
    generation: Rc<()>,
    depth: Rc<Cell<usize>>,
}

impl RequestLifecycle {
    pub(super) fn register(&mut self, request_id: UUID4) {
        self.requests.insert(request_id, RequestState::Pending);
    }

    pub(super) fn register_group(
        &mut self,
        request_ids: Vec<UUID4>,
        request_id: Option<UUID4>,
        handler: Option<TypedHandler<DataResponseEnvelope>>,
    ) -> anyhow::Result<UUID4> {
        anyhow::ensure!(!request_ids.is_empty(), "Request group must not be empty");
        let mut seen = AHashSet::new();

        for id in &request_ids {
            anyhow::ensure!(seen.insert(*id), "Duplicate request ID in group: {id}");
            anyhow::ensure!(self.requests.contains_key(id), "Unknown request ID: {id}");
        }

        let group_id = request_id.unwrap_or_default();
        anyhow::ensure!(
            !self.requests.contains_key(&group_id),
            "Request ID already registered: {group_id}",
        );
        let mut remaining = 0;

        for id in &request_ids {
            if self.is_pending(id) {
                self.parents.entry(*id).or_default().push(group_id);
                remaining += 1;
            }
        }

        self.groups.insert(
            group_id,
            RequestGroup {
                children: request_ids,
                remaining,
                handler,
            },
        );

        self.register(group_id);
        if remaining == 0 {
            self.ready.push_back(group_id);
        }

        Ok(group_id)
    }

    pub(super) fn is_pending(&self, request_id: &UUID4) -> bool {
        matches!(self.requests.get(request_id), Some(RequestState::Pending))
    }

    pub(super) fn pending(&self) -> Vec<UUID4> {
        self.requests
            .iter()
            .filter_map(|(id, state)| matches!(state, RequestState::Pending).then_some(*id))
            .collect()
    }

    #[cfg(feature = "python")]
    pub(super) fn is_current(&self, scope: &ResponseScope) -> bool {
        Rc::ptr_eq(&self.generation, &scope.generation)
    }

    pub(super) fn begin(&self, request_id: &UUID4) -> Option<ResponseScope> {
        if !self.is_pending(request_id) {
            return None;
        }

        self.depth.set(self.depth.get() + 1);
        Some(ResponseScope {
            generation: Rc::clone(&self.generation),
            depth: Some(Rc::clone(&self.depth)),
        })
    }

    pub(super) fn complete(
        &mut self,
        envelope: &DataResponseEnvelope,
        scope: &ResponseScope,
    ) -> bool {
        if !Rc::ptr_eq(&self.generation, &scope.generation)
            || !self.is_pending(envelope.correlation_id())
        {
            return false;
        }

        if !envelope.completed() {
            return true;
        }

        let id = *envelope.correlation_id();
        let outcome = RequestState::Complete(envelope.failure_reason().map(str::to_owned));
        self.requests.insert(id, outcome);
        for parent in self.parents.remove(&id).unwrap_or_default() {
            let group = self
                .groups
                .get_mut(&parent)
                .expect("registered request group");
            group.remaining -= 1;
            if group.remaining == 0 {
                self.ready.push_back(parent);
            }
        }

        true
    }

    pub(super) fn next_group_response(
        &mut self,
    ) -> Option<(
        DataResponseEnvelope,
        Option<TypedHandler<DataResponseEnvelope>>,
    )> {
        if self.depth.get() != 0 {
            return None;
        }

        let id = self.ready.pop_front()?;
        let group = self.groups.remove(&id).expect("ready request group");

        let failure = group.children.iter().find_map(|child| {
            match self.requests.get(child).expect("registered child request") {
                RequestState::Complete(Some(reason)) => Some(format!("Request {child}: {reason}")),
                RequestState::Complete(None) => None,
                RequestState::Pending => unreachable!("ready group has no pending children"),
            }
        });

        Some((DataResponseEnvelope::terminal(id, failure), group.handler))
    }

    pub(super) fn clear(&mut self) -> (Vec<UUID4>, Vec<TypedHandler<DataResponseEnvelope>>) {
        let pending = self
            .requests
            .iter()
            .filter_map(|(id, state)| {
                (matches!(state, RequestState::Pending) && !self.groups.contains_key(id))
                    .then_some(*id)
            })
            .collect();

        self.requests.clear();
        let handlers = self
            .groups
            .drain()
            .filter_map(|(_, group)| group.handler)
            .collect();
        self.parents.clear();
        self.ready.clear();
        self.generation = Rc::new(());
        (pending, handlers)
    }
}

impl Clone for RequestLifecycle {
    fn clone(&self) -> Self {
        Self {
            requests: self.requests.clone(),
            groups: self.groups.clone(),
            parents: self.parents.clone(),
            ready: self.ready.clone(),
            generation: Rc::new(()),
            depth: Rc::default(),
        }
    }
}

#[derive(Debug, Clone)]
enum RequestState {
    Pending,
    Complete(Option<String>),
}

#[derive(Debug, Clone)]
struct RequestGroup {
    children: Vec<UUID4>,
    remaining: usize,
    handler: Option<TypedHandler<DataResponseEnvelope>>,
}

pub(crate) struct ResponseScope {
    generation: Rc<()>,
    depth: Option<Rc<Cell<usize>>>,
}

impl ResponseScope {
    pub(crate) fn leave(&mut self) {
        if let Some(depth) = self.depth.take() {
            depth.set(depth.get() - 1);
        }
    }
}

impl Drop for ResponseScope {
    fn drop(&mut self) {
        self.leave();
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn groups_wait_for_all_children_and_preserve_failure_order() {
        let mut lifecycle = RequestLifecycle::default();
        let first = UUID4::new();
        let second = UUID4::new();
        lifecycle.register(first);
        lifecycle.register(second);
        let group = lifecycle
            .register_group(vec![first, second], None, None)
            .unwrap();
        let overlapping = lifecycle.register_group(vec![second], None, None).unwrap();
        let parent = lifecycle
            .register_group(vec![group, overlapping], None, None)
            .unwrap();

        let scope = lifecycle.begin(&second).unwrap();
        let response = DataResponseEnvelope::terminal(second, Some("second failed".to_owned()));
        assert!(lifecycle.complete(&response, &scope));
        assert!(!lifecycle.complete(&response, &scope));
        assert!(lifecycle.next_group_response().is_none());
        drop(scope);
        let response = lifecycle.next_group_response().unwrap().0;
        assert_eq!(*response.correlation_id(), overlapping);
        assert_eq!(
            response.failure_reason(),
            Some(format!("Request {second}: second failed").as_str())
        );
        let scope = lifecycle.begin(&overlapping).unwrap();
        assert!(lifecycle.complete(&response, &scope));
        drop(scope);
        assert!(lifecycle.next_group_response().is_none());

        let scope = lifecycle.begin(&first).unwrap();
        let response = DataResponseEnvelope::terminal(first, Some("first failed".to_owned()));
        assert!(lifecycle.complete(&response, &scope));
        drop(scope);
        let response = lifecycle.next_group_response().unwrap().0;
        assert_eq!(*response.correlation_id(), group);
        assert_eq!(
            response.failure_reason(),
            Some(format!("Request {first}: first failed").as_str())
        );
        assert!(response.completed());
        assert!(response.response().is_none());
        assert_eq!(response.sub_correlation_id(), None);
        let scope = lifecycle.begin(&group).unwrap();
        assert!(lifecycle.complete(&response, &scope));
        drop(scope);
        let response = lifecycle.next_group_response().unwrap().0;
        assert_eq!(*response.correlation_id(), parent);
        assert_eq!(
            response.failure_reason(),
            Some(format!("Request {group}: Request {first}: first failed").as_str())
        );
        assert!(lifecycle.next_group_response().is_none());
    }

    #[rstest]
    fn completed_children_remain_joinable_and_scopes_defer_notifications() {
        let mut lifecycle = RequestLifecycle::default();
        let child = UUID4::new();
        lifecycle.register(child);
        let scope = lifecycle.begin(&child).unwrap();
        let response = DataResponseEnvelope::terminal(child, None);
        assert!(lifecycle.complete(&response, &scope));
        let first = lifecycle.register_group(vec![child], None, None).unwrap();
        let second = lifecycle.register_group(vec![child], None, None).unwrap();
        assert!(lifecycle.next_group_response().is_none());
        drop(scope);

        for id in [first, second] {
            let response = lifecycle.next_group_response().unwrap().0;
            assert_eq!(*response.correlation_id(), id);
            assert!(response.completed());
            assert_eq!(response.failure_reason(), None);
            assert!(response.response().is_none());
            assert_eq!(response.sub_correlation_id(), None);
            let scope = lifecycle.begin(&id).unwrap();
            assert!(lifecycle.complete(&response, &scope));
        }

        assert!(lifecycle.next_group_response().is_none());
    }

    #[rstest]
    #[case::empty(0)]
    #[case::duplicate(1)]
    #[case::unknown(2)]
    fn invalid_group_has_no_effects(#[case] case: u8) {
        let mut lifecycle = RequestLifecycle::default();
        let child = UUID4::new();
        let unknown = UUID4::new();
        lifecycle.register(child);

        let (ids, expected) = match case {
            0 => (vec![], "Request group must not be empty".to_owned()),
            1 => (
                vec![child, child],
                format!("Duplicate request ID in group: {child}"),
            ),
            2 => (
                vec![child, unknown],
                format!("Unknown request ID: {unknown}"),
            ),
            _ => unreachable!(),
        };

        assert_eq!(
            lifecycle
                .register_group(ids, None, None)
                .unwrap_err()
                .to_string(),
            expected
        );
        assert_eq!(lifecycle.requests.len(), 1);
        assert!(lifecycle.is_pending(&child));
        assert!(lifecycle.groups.is_empty());
        assert!(lifecycle.parents.is_empty());
        assert!(lifecycle.ready.is_empty());
    }

    #[rstest]
    fn clear_invalidates_scopes_and_discards_ready_groups() {
        let mut lifecycle = RequestLifecycle::default();
        let child = UUID4::new();
        lifecycle.register(child);
        let group = lifecycle.register_group(vec![child], None, None).unwrap();
        let scope = lifecycle.begin(&child).unwrap();
        let response = DataResponseEnvelope::terminal(child, None);
        assert!(lifecycle.complete(&response, &scope));
        assert!(lifecycle.clear().0.is_empty());
        assert!(!lifecycle.is_pending(&group));
        lifecycle.register(child);
        assert!(!lifecycle.complete(&response, &scope));
        assert!(lifecycle.is_pending(&child));
        drop(scope);
        assert!(lifecycle.next_group_response().is_none());
        assert!(lifecycle.groups.is_empty());
        assert!(lifecycle.parents.is_empty());
    }

    #[rstest]
    fn replacement_lifecycle_rejects_old_response_scope() {
        let mut original = RequestLifecycle::default();
        let child = UUID4::new();
        original.register(child);
        let scope = original.begin(&child).unwrap();
        let mut replacement = original.clone();
        let response = DataResponseEnvelope::terminal(child, Some("old failure".to_owned()));
        assert!(!replacement.complete(&response, &scope));
        assert!(replacement.is_pending(&child));
        let current_scope = replacement.begin(&child).unwrap();
        let current = DataResponseEnvelope::terminal(child, None);
        assert!(replacement.complete(&current, &current_scope));
        drop(current_scope);
        let group = replacement.register_group(vec![child], None, None).unwrap();
        let response = replacement.next_group_response().unwrap().0;
        assert_eq!(*response.correlation_id(), group);
        assert_eq!(response.failure_reason(), None);
        assert!(original.is_pending(&child));
    }
}
