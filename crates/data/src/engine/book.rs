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

use std::{cell::RefCell, num::NonZeroUsize, rc::Rc};

use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    messages::data::{SubscribeBookSnapshots, SubscribeCommand},
    msgbus::{self, Handler, MStr, Topic, switchboard},
    timer::TimeEvent,
};
use nautilus_model::{
    data::{OrderBookDeltas, OrderBookDepth, QuoteTick},
    enums::{BookType, InstrumentClass},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::Instrument,
    orderbook::OrderBook,
};
use ustr::Ustr;

use super::{
    BookDeltasResponse, BookDepthResponse, DataEngine, DurationNanos, FAILED, OrderBookDelta,
    PARAMS_IS_PARENT, Params, RecordFlag, SubscribeBookDeltas, SubscribeBookDepth, SubscriptionKey,
    TimeEventCallback, TypedHandler, UUID4, UnsubscribeBookDeltas, UnsubscribeBookDepth,
    UnsubscribeBookSnapshots, UnsubscribeCommand, is_parent_subscription,
    log_error_on_cache_insert,
};

impl DataEngine {
    /// Returns all instrument IDs for which book delta subscriptions exist.
    #[must_use]
    pub fn subscribed_book_deltas(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_book_deltas)
    }

    /// Returns all instrument IDs for which book depth subscriptions exist.
    #[must_use]
    pub fn subscribed_book_depth(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_book_depth)
    }

    /// Returns all instrument IDs for which book snapshot subscriptions exist.
    #[must_use]
    pub fn subscribed_book_snapshots(&self) -> Vec<InstrumentId> {
        self.book_snapshot_counts
            .keys()
            .map(|(instrument_id, _)| *instrument_id)
            .collect()
    }

    pub(super) fn subscribe_book_deltas(
        &mut self,
        cmd: &SubscribeBookDeltas,
    ) -> anyhow::Result<bool> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        let had_deltas =
            self.has_book_delta_subscription_key(cmd.instrument_id, cmd.client_id, cmd.venue);

        self.retain_book_subscription(SubscribeCommand::BookDeltas(cmd.clone()))?;

        self.increment_book_delta_subscription(cmd.instrument_id, cmd.client_id, cmd.venue);

        Ok(!had_deltas)
    }

    pub(super) fn subscribe_book_depth(
        &mut self,
        cmd: &SubscribeBookDepth,
    ) -> anyhow::Result<bool> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDepth` data");
        }

        let had_depth =
            self.has_book_depth_subscription_key(cmd.instrument_id, cmd.client_id, cmd.venue);

        self.retain_book_subscription(SubscribeCommand::BookDepth(cmd.clone()))?;

        self.increment_book_depth_subscription(cmd.instrument_id, cmd.client_id, cmd.venue);

        Ok(!had_depth)
    }

    pub(super) fn subscribe_book_snapshots(
        &mut self,
        cmd: &SubscribeBookSnapshots,
    ) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        let parent = resolve_parent_components(&cmd.instrument_id, cmd.params.as_ref())?;

        let had_snapshots = self.has_book_snapshot_subscriptions(&cmd.instrument_id);

        self.retain_book_subscription(SubscribeCommand::BookSnapshots(cmd.clone()))?;

        self.increment_book_snapshot_subscription(cmd, parent);

        if !had_snapshots {
            self.book_snapshot_sources.insert(
                cmd.instrument_id,
                BookSnapshotSource {
                    command: cmd.clone(),
                    client_command: SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
                        cmd.instrument_id,
                        cmd.book_type,
                        cmd.client_id,
                        cmd.venue,
                        UUID4::new(),
                        cmd.ts_init,
                        cmd.depth,
                        true, // managed
                        Some(cmd.command_id),
                        cmd.params.clone(),
                    )),
                },
            );
        }

        let source = self
            .book_snapshot_sources
            .get(&cmd.instrument_id)
            .cloned()
            .expect("snapshot source command must exist after increment");
        self.subscribe_book_snapshot_source(&source.command, source.client_command);

        Ok(())
    }

    fn subscribe_book_snapshot_source(
        &mut self,
        cmd: &SubscribeBookSnapshots,
        client_command: SubscribeCommand,
    ) {
        if let Some(client_id) = cmd.client_id.as_ref()
            && self.external_clients.contains(client_id)
        {
            if self.config.debug {
                log::debug!("Skipping subscribe command for external client {client_id}: {cmd:?}");
            }

            return;
        }

        log::debug!(
            "Forwarding BookSnapshots as BookDeltas for {}, client_id={:?}, venue={:?}",
            cmd.instrument_id,
            cmd.client_id,
            cmd.venue,
        );

        if let Some(client) = self.get_command_client(cmd.client_id.as_ref(), cmd.venue.as_ref()) {
            log::debug!(
                "Calling client.execute_subscribe for BookDeltas: {}",
                cmd.instrument_id
            );
            client.execute_subscribe(client_command);
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id,
                cmd.venue,
            );
        }
    }

    pub(super) fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> bool {
        match self.decrement_book_delta_subscription(cmd.instrument_id, cmd.client_id, cmd.venue) {
            BookDeltasUnsubscribeResult::NotSubscribed => {
                log::warn!("Cannot unsubscribe from `OrderBookDeltas` data: not subscribed");
                return false;
            }
            BookDeltasUnsubscribeResult::Decremented => return false,
            BookDeltasUnsubscribeResult::Removed => {}
        }

        true
    }

    pub(super) fn unsubscribe_book_depth(&mut self, cmd: &UnsubscribeBookDepth) -> bool {
        match self.decrement_book_depth_subscription(cmd.instrument_id, cmd.client_id, cmd.venue) {
            BookDeltasUnsubscribeResult::NotSubscribed => {
                log::warn!("Cannot unsubscribe from `OrderBookDepth` data: not subscribed");
                return false;
            }
            BookDeltasUnsubscribeResult::Decremented => return false,
            BookDeltasUnsubscribeResult::Removed => {}
        }

        true
    }

    pub(super) fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) {
        match self.decrement_book_snapshot_subscription(cmd.instrument_id, cmd.interval_ms) {
            BookSnapshotUnsubscribeResult::NotSubscribed => {
                log::warn!("Cannot unsubscribe from `OrderBook` snapshots: not subscribed");
                return;
            }
            BookSnapshotUnsubscribeResult::Decremented => return,
            BookSnapshotUnsubscribeResult::Removed => {}
        }

        if self.has_book_snapshot_subscriptions(&cmd.instrument_id) {
            return;
        }

        let Some(source) = self.book_snapshot_sources.remove(&cmd.instrument_id) else {
            log::error!(
                "Cannot release order book snapshot source for {}: command not retained",
                cmd.instrument_id,
            );
            return;
        };

        if let Some(client_id) = source.command.client_id.as_ref()
            && self.external_clients.contains(client_id)
        {
            return;
        }

        if let Some(client) = self.get_command_client(
            source.command.client_id.as_ref(),
            source.command.venue.as_ref(),
        ) {
            let deltas_cmd = UnsubscribeBookDeltas::new(
                source.command.instrument_id,
                source.command.client_id,
                source.command.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(source.command.command_id),
                source.command.params,
            );
            client.execute_unsubscribe(&UnsubscribeCommand::BookDeltas(deltas_cmd));
        }
    }

    fn retain_book_subscription(&mut self, command: SubscribeCommand) -> anyhow::Result<()> {
        let instrument_id = match &command {
            SubscribeCommand::BookDeltas(cmd) => cmd.instrument_id,
            SubscribeCommand::BookDepth(cmd) => cmd.instrument_id,
            SubscribeCommand::BookSnapshots(cmd) => cmd.instrument_id,
            _ => unreachable!("only book subscriptions are retained"),
        };

        let parent = resolve_parent_components(&instrument_id, command.params())?;

        let targets = if let Some((root, class)) = parent {
            self.cache
                .borrow()
                .instruments_by_parent(&instrument_id.venue, &root, class)
                .iter()
                .map(|instrument| instrument.id())
                .collect()
        } else {
            vec![instrument_id]
        };

        let client_id = self
            .get_command_client(command.client_id(), command.venue())
            .map(|client| client.client_id);

        let subscription = Rc::new(BookSubscriptionOwner {
            command,
            client_id,
            targets,
        });

        let mut params = subscription.command.params().cloned().unwrap_or_default();
        params.shift_remove(PARAMS_IS_PARENT);

        for active in subscription
            .targets
            .iter()
            .filter_map(|id| self.book_subscriptions.get(id))
            .flat_map(|book| &book.owners)
        {
            if !subscription.managed()
                && !active.managed()
                && subscription.client_id != active.client_id
            {
                continue;
            }

            if subscription.is_depth() != active.is_depth() {
                anyhow::ensure!(
                    !subscription.managed() || !active.managed(),
                    "Conflicting managed book source for {instrument_id}: deltas and depth cannot both manage the same book; use managed=false for the other subscription"
                );
                continue;
            }

            let mut active_params = active.command.params().cloned().unwrap_or_default();
            active_params.shift_remove(PARAMS_IS_PARENT);
            anyhow::ensure!(
                subscription.client_id == active.client_id
                    && subscription.config() == active.config()
                    && params == active_params,
                "Conflicting book subscription for {instrument_id}: shared book sources must use the same client, book type, depth, and parameters"
            );
        }

        if subscription.managed() {
            self.setup_book_updater(
                &subscription.targets,
                subscription.config().0,
                subscription.is_depth(),
            )?;
        }

        for target_id in &subscription.targets {
            self.book_subscriptions
                .entry(*target_id)
                .or_default()
                .owners
                .push(subscription.clone());
        }

        self.book_subscription_owners
            .entry(instrument_id)
            .or_default()
            .push(subscription);
        Ok(())
    }

    pub(super) fn release_book_subscription(&mut self, command: &UnsubscribeCommand) -> bool {
        let key = SubscriptionKey::from_unsubscribe(command);

        let instrument_id = match command {
            UnsubscribeCommand::BookDeltas(cmd) => cmd.instrument_id,
            UnsubscribeCommand::BookDepth(cmd) => cmd.instrument_id,
            UnsubscribeCommand::BookSnapshots(cmd) => cmd.instrument_id,
            _ => unreachable!("only book subscriptions are released"),
        };

        let Some(owners) = self.book_subscription_owners.get_mut(&instrument_id) else {
            return false;
        };

        let index = owners.iter().position(|subscription| {
            SubscriptionKey::from_subscribe(&subscription.command) == key
                && subscription.command.client_id() == command.client_id()
                && subscription.command.venue() == command.venue()
                && command
                    .correlation_id()
                    .is_none_or(|id| subscription.command.command_id() == id)
        });

        let Some(index) = index else {
            return false;
        };

        let subscription = owners.remove(index);
        if owners.is_empty() {
            self.book_subscription_owners.remove(&instrument_id);
        }

        for &target_id in &subscription.targets {
            let book = self
                .book_subscriptions
                .get_mut(&target_id)
                .expect("retained book subscription");
            book.owners
                .retain(|owner| !Rc::ptr_eq(owner, &subscription));
            let managed = book.owners.iter().any(|owner| owner.managed());
            if book.owners.is_empty() {
                self.book_subscriptions.remove(&target_id);
            }

            if managed {
                continue;
            }

            let Some(updater) = self.book_updaters.remove(&target_id) else {
                continue;
            };

            let deltas_handler: TypedHandler<OrderBookDeltas> = TypedHandler::new(updater.clone());
            let depth_handler: TypedHandler<OrderBookDepth> = TypedHandler::new(updater);
            msgbus::unsubscribe_book_deltas(
                switchboard::get_book_deltas_topic(target_id).into(),
                &deltas_handler,
            );
            msgbus::unsubscribe_book_depth(
                switchboard::get_book_depth_topic(target_id).into(),
                &depth_handler,
            );
        }

        true
    }

    fn has_book_snapshot_subscriptions(&self, instrument_id: &InstrumentId) -> bool {
        self.book_snapshot_counts
            .keys()
            .any(|(id, _)| id == instrument_id)
    }

    fn has_book_delta_subscription_key(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) -> bool {
        self.book_deltas_counts
            .contains_key(&(instrument_id, client_id, venue))
    }

    fn has_book_depth_subscription_key(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) -> bool {
        self.book_depth_counts
            .contains_key(&(instrument_id, client_id, venue))
    }

    fn increment_book_delta_subscription(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) {
        let key = (instrument_id, client_id, venue);

        if let Some(count) = self.book_deltas_counts.get_mut(&key) {
            *count += 1;
        } else {
            self.book_deltas_counts.insert(key, 1);
        }
    }

    fn decrement_book_delta_subscription(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) -> BookDeltasUnsubscribeResult {
        let key = (instrument_id, client_id, venue);

        let Some(count) = self.book_deltas_counts.get_mut(&key) else {
            return BookDeltasUnsubscribeResult::NotSubscribed;
        };

        if *count > 1 {
            *count -= 1;
            return BookDeltasUnsubscribeResult::Decremented;
        }

        self.book_deltas_counts.shift_remove(&key);
        BookDeltasUnsubscribeResult::Removed
    }

    fn increment_book_depth_subscription(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) {
        let key = (instrument_id, client_id, venue);
        *self.book_depth_counts.entry(key).or_insert(0) += 1;
    }

    fn decrement_book_depth_subscription(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) -> BookDeltasUnsubscribeResult {
        let key = (instrument_id, client_id, venue);

        let Some(count) = self.book_depth_counts.get_mut(&key) else {
            return BookDeltasUnsubscribeResult::NotSubscribed;
        };

        if *count > 1 {
            *count -= 1;
            return BookDeltasUnsubscribeResult::Decremented;
        }

        self.book_depth_counts.shift_remove(&key);
        BookDeltasUnsubscribeResult::Removed
    }

    fn increment_book_snapshot_subscription(
        &mut self,
        cmd: &SubscribeBookSnapshots,
        parent: Option<(Ustr, InstrumentClass)>,
    ) -> bool {
        let key = (cmd.instrument_id, cmd.interval_ms);

        if let Some(count) = self.book_snapshot_counts.get_mut(&key) {
            *count += 1;
            return false;
        }

        self.book_snapshot_counts.insert(key, 1);

        let snapshot_infos = if let Some(snapshot_infos) = self.book_intervals.get(&cmd.interval_ms)
        {
            snapshot_infos.clone()
        } else {
            let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));
            self.book_intervals
                .insert(cmd.interval_ms, snapshot_infos.clone());
            self.schedule_book_snapshotter(cmd.interval_ms, snapshot_infos.clone());
            snapshot_infos
        };

        let topic = switchboard::get_book_snapshots_topic(cmd.instrument_id, cmd.interval_ms);

        let snap_info = BookSnapshotInfo {
            instrument_id: cmd.instrument_id,
            venue: cmd.instrument_id.venue,
            parent,
            topic,
            interval_ms: cmd.interval_ms,
        };

        snapshot_infos
            .borrow_mut()
            .insert(cmd.instrument_id, snap_info);

        true
    }

    fn decrement_book_snapshot_subscription(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: NonZeroUsize,
    ) -> BookSnapshotUnsubscribeResult {
        let key = (instrument_id, interval_ms);

        let Some(count) = self.book_snapshot_counts.get_mut(&key) else {
            return BookSnapshotUnsubscribeResult::NotSubscribed;
        };

        if *count > 1 {
            *count -= 1;
            return BookSnapshotUnsubscribeResult::Decremented;
        }

        self.book_snapshot_counts.shift_remove(&key);

        let remove_interval = if let Some(snapshot_infos) = self.book_intervals.get(&interval_ms) {
            let mut snapshot_infos = snapshot_infos.borrow_mut();
            snapshot_infos.shift_remove(&instrument_id);
            snapshot_infos.is_empty()
        } else {
            false
        };

        if remove_interval {
            self.book_intervals.remove(&interval_ms);

            if let Some(snapshotter) = self.book_snapshotters.remove(&interval_ms) {
                let timer_name = snapshotter.timer_name;
                let mut clock = self.clock.borrow_mut();
                if clock.timer_exists(&timer_name) {
                    clock.cancel_timer(&timer_name);
                }
            }
        }

        BookSnapshotUnsubscribeResult::Removed
    }

    fn schedule_book_snapshotter(
        &mut self,
        interval_ms: NonZeroUsize,
        snapshot_infos: BookSnapshotInfos,
    ) {
        let interval_ms_u64 =
            u64::try_from(interval_ms.get()).expect("Snapshot interval exceeds u64");
        let interval_ns = DurationNanos::from_millis(interval_ms_u64);
        let now_ns = self.clock.borrow().timestamp_ns();
        let start_time_ns = now_ns
            .floor(interval_ns)
            .checked_add(interval_ns)
            .expect("Book snapshot timer start exceeds UnixNanos range");

        let snapshotter = Rc::new(BookSnapshotter::new(
            interval_ms,
            snapshot_infos,
            self.cache.clone(),
        ));
        let timer_name = snapshotter.timer_name;
        let snapshotter_callback = snapshotter.clone();
        let callback_fn: Rc<dyn Fn(TimeEvent)> =
            Rc::new(move |event| snapshotter_callback.snapshot(event));
        let callback = TimeEventCallback::from(callback_fn);

        self.clock
            .borrow_mut()
            .set_timer_ns(
                &timer_name,
                interval_ns,
                Some(start_time_ns),
                None,
                Some(callback),
                None,
                None,
            )
            .expect(FAILED);

        self.book_snapshotters.insert(interval_ms, snapshotter);
    }

    // Skip cache writes that would regress a book a `BookUpdater` is maintaining.
    // Unmanaged subscriptions don't install a `BookUpdater`, so they don't gate writes.
    fn cache_is_owned_by_live_subscription(&self, instrument_id: &InstrumentId) -> bool {
        self.book_updaters.contains_key(instrument_id)
    }

    pub(super) fn handle_book_response(&self, book: &OrderBook) {
        if self.cache_is_owned_by_live_subscription(&book.instrument_id) {
            log::debug!(
                "Skipping cache write for order book {}: live subscription owns the book",
                book.instrument_id,
            );
            return;
        }

        log::debug!("Adding order book {} to cache", book.instrument_id);

        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_order_book(book.clone())
        {
            log_error_on_cache_insert(&e);
        }
    }

    pub(super) fn handle_book_deltas_response(&self, resp: &BookDeltasResponse) {
        if !self.cache_is_owned_by_live_subscription(&resp.instrument_id) {
            let mut cache = self.cache.as_ref().borrow_mut();
            if let Some(book) = cache.order_book_mut(&resp.instrument_id) {
                for delta in &resp.data {
                    if let Err(e) = book.apply_delta(delta) {
                        log::error!("Failed to apply historical delta to cache: {e}");
                    }
                }
            } else {
                log::debug!(
                    "Skipping cache write for {} historical deltas on {}: no cache book yet",
                    resp.data.len(),
                    resp.instrument_id,
                );
            }
        }

        // Group deltas by `F_LAST` so each published batch preserves the original event
        // boundary and metadata (timestamps and sequence from the closing delta), matching
        // the live `handle_delta` buffering semantic. Collapsing the whole response into
        // one batch would surface a synthetic event with the trailing delta's flags only.
        if resp.data.is_empty() {
            return;
        }

        let topic = switchboard::get_pipeline_book_deltas_topic(resp.instrument_id);
        let mut frame: Vec<OrderBookDelta> = Vec::new();

        for delta in &resp.data {
            frame.push(*delta);
            if RecordFlag::F_LAST.matches(delta.flags) {
                let batch = OrderBookDeltas::new(resp.instrument_id, std::mem::take(&mut frame));
                msgbus::publish_deltas(topic, &batch);
            }
        }

        if !frame.is_empty() {
            let batch = OrderBookDeltas::new(resp.instrument_id, frame);
            msgbus::publish_deltas(topic, &batch);
        }
    }

    pub(super) fn handle_book_depth_response(&self, resp: &BookDepthResponse) {
        let topic = switchboard::get_pipeline_book_depth_topic(resp.instrument_id);

        for depth in &resp.data {
            msgbus::publish_depth(topic, depth);
        }
    }

    fn setup_book_updater(
        &mut self,
        target_ids: &[InstrumentId],
        book_type: BookType,
        depth: bool,
    ) -> anyhow::Result<()> {
        {
            let mut cache = self.cache.borrow_mut();

            for target_id in target_ids {
                if !cache.has_order_book(target_id) {
                    cache.add_order_book(OrderBook::new(*target_id, book_type))?;
                }
            }
        }

        for target_id in target_ids {
            let updater = self
                .book_updaters
                .entry(*target_id)
                .or_insert_with(|| {
                    Rc::new(BookUpdater::new(
                        target_id,
                        self.cache.clone(),
                        self.config.emit_quotes_from_book,
                    ))
                })
                .clone();

            if depth {
                msgbus::subscribe_book_depth(
                    switchboard::get_book_depth_topic(*target_id).into(),
                    TypedHandler::new(updater),
                    Some(self.msgbus_priority),
                );
            } else {
                msgbus::subscribe_book_deltas(
                    switchboard::get_book_deltas_topic(*target_id).into(),
                    TypedHandler::new(updater),
                    Some(self.msgbus_priority),
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct BookSubscription {
    pub(super) owners: Vec<Rc<BookSubscriptionOwner>>,
}

#[derive(Debug)]
pub(super) struct BookSubscriptionOwner {
    pub(super) command: SubscribeCommand,
    pub(super) client_id: Option<ClientId>,
    pub(super) targets: Vec<InstrumentId>,
}

impl BookSubscriptionOwner {
    pub(super) fn managed(&self) -> bool {
        match &self.command {
            SubscribeCommand::BookDeltas(cmd) => cmd.managed,
            SubscribeCommand::BookDepth(cmd) => cmd.managed,
            SubscribeCommand::BookSnapshots(_) => true,
            _ => unreachable!("only book subscriptions are retained"),
        }
    }

    pub(super) fn is_depth(&self) -> bool {
        matches!(self.command, SubscribeCommand::BookDepth(_))
    }

    pub(super) fn config(&self) -> (BookType, Option<NonZeroUsize>) {
        match &self.command {
            SubscribeCommand::BookDeltas(cmd) => (cmd.book_type, cmd.depth),
            SubscribeCommand::BookDepth(cmd) => (cmd.book_type, cmd.depth),
            SubscribeCommand::BookSnapshots(cmd) => (cmd.book_type, cmd.depth),
            _ => unreachable!("only book subscriptions are retained"),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct BookSnapshotSource {
    pub(super) command: SubscribeBookSnapshots,
    pub(super) client_command: SubscribeCommand,
}

/// Contains information for creating snapshots of specific order books.
#[derive(Clone, Debug)]
pub struct BookSnapshotInfo {
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    /// Parent expansion components `(root, class)` when this snapshot subscription
    /// targets a parent symbol. `None` for concrete (exact-instrument) subscriptions.
    pub parent: Option<(Ustr, InstrumentClass)>,
    pub topic: MStr<Topic>,
    pub interval_ms: NonZeroUsize,
}

/// Reference-counted map of per-instrument book snapshot descriptors.
///
/// Shared between the engine (which populates it on subscribe) and the
/// [`BookSnapshotter`] timer callback (which iterates it on each tick).
pub(crate) type BookSnapshotInfos = Rc<RefCell<IndexMap<InstrumentId, BookSnapshotInfo>>>;

/// Reference count key for a book snapshot subscription.
pub(crate) type BookSnapshotKey = (InstrumentId, NonZeroUsize);

/// Outcome of decrementing a book snapshot subscription.
pub(crate) enum BookSnapshotUnsubscribeResult {
    /// No matching subscription was found.
    NotSubscribed,
    /// The reference count was decremented but other consumers remain.
    Decremented,
    /// The last consumer was removed; tear down associated state.
    Removed,
}

/// Reference count key for a book deltas subscription.
pub(crate) type BookDeltasKey = (InstrumentId, Option<ClientId>, Option<Venue>);

/// Outcome of decrementing a book deltas subscription.
pub(crate) enum BookDeltasUnsubscribeResult {
    /// No matching subscription was found.
    NotSubscribed,
    /// The reference count was decremented but other consumers remain.
    Decremented,
    /// The last consumer was removed; tear down associated state.
    Removed,
}

/// Handles order book updates and delta processing for a specific instrument.
///
/// The `BookUpdater` processes incoming order book deltas and maintains
/// the current state of an order book. It can handle both incremental
/// updates and full snapshots for the instrument it's assigned to.
#[derive(Debug)]
pub struct BookUpdater {
    pub id: Ustr,
    pub instrument_id: InstrumentId,
    pub cache: Rc<RefCell<Cache>>,
    pub emit_quotes_from_book: bool,
}

impl BookUpdater {
    /// Creates a new [`BookUpdater`] instance.
    pub fn new(
        instrument_id: &InstrumentId,
        cache: Rc<RefCell<Cache>>,
        emit_quotes_from_book: bool,
    ) -> Self {
        Self {
            id: Ustr::from(&format!("{}-{}", stringify!(BookUpdater), instrument_id)),
            instrument_id: *instrument_id,
            cache,
            emit_quotes_from_book,
        }
    }
}

impl Handler<OrderBookDeltas> for BookUpdater {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, deltas: &OrderBookDeltas) {
        let mut emit: Option<QuoteTick> = None;
        {
            let mut cache = self.cache.borrow_mut();
            if let Some(book) = cache.order_book_mut(&deltas.instrument_id) {
                if let Err(e) = book.apply_deltas(deltas) {
                    log::error!("Failed to apply deltas: {e}");
                    return;
                }

                if self.emit_quotes_from_book {
                    emit = derive_quote_from_book(book);
                }
            }
        }

        if let Some(quote) = emit {
            publish_quote_if_changed(&self.cache, quote);
        }
    }
}

impl Handler<OrderBookDepth> for BookUpdater {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, depth: &OrderBookDepth) {
        let mut emit: Option<QuoteTick> = None;
        {
            let mut cache = self.cache.borrow_mut();
            if let Some(book) = cache.order_book_mut(&depth.instrument_id) {
                if let Err(e) = book.apply_depth(depth) {
                    log::error!("Failed to apply depth: {e}");
                    return;
                }

                if self.emit_quotes_from_book {
                    emit = derive_quote_from_book(book);
                }
            }
        }

        if let Some(quote) = emit {
            publish_quote_if_changed(&self.cache, quote);
        }
    }
}

fn derive_quote_from_book(book: &OrderBook) -> Option<QuoteTick> {
    let bid_price = book.best_bid_price()?;
    let ask_price = book.best_ask_price()?;
    let bid_size = book.best_bid_size()?;
    let ask_size = book.best_ask_size()?;

    if bid_size.is_zero() || ask_size.is_zero() {
        return None;
    }

    Some(QuoteTick::new(
        book.instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        book.ts_last,
        book.ts_last,
    ))
}

/// Publishes the derived `QuoteTick` if top-of-book changed.
///
/// Writes to cache and republishes only when bid/ask price or size differs
/// from the cached quote.
pub(crate) fn publish_quote_if_changed(cache: &Rc<RefCell<Cache>>, quote: QuoteTick) {
    let publish = {
        let cache_ref = cache.borrow();
        match cache_ref.quote(&quote.instrument_id) {
            None => true,
            Some(last) => {
                last.bid_price != quote.bid_price
                    || last.ask_price != quote.ask_price
                    || last.bid_size != quote.bid_size
                    || last.ask_size != quote.ask_size
            }
        }
    };

    if !publish {
        return;
    }

    if let Err(e) = cache.borrow_mut().add_quote(quote) {
        log::error!("Error on cache insert: {e}");
    }

    let topic = switchboard::get_quotes_topic(quote.instrument_id);
    msgbus::publish_quote(topic, &quote);
}

/// Creates periodic snapshots of order books at configured intervals.
///
/// The `BookSnapshotter` generates order book snapshots on timer events,
/// publishing them as market data. This is useful for providing periodic
/// full order book state updates in addition to incremental delta updates.
#[derive(Debug)]
pub struct BookSnapshotter {
    pub timer_name: Ustr,
    pub interval_ms: NonZeroUsize,
    pub snapshot_infos: Rc<RefCell<IndexMap<InstrumentId, BookSnapshotInfo>>>,
    pub cache: Rc<RefCell<Cache>>,
}

impl BookSnapshotter {
    /// Creates a new [`BookSnapshotter`] instance.
    pub fn new(
        interval_ms: NonZeroUsize,
        snapshot_infos: Rc<RefCell<IndexMap<InstrumentId, BookSnapshotInfo>>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        let timer_name = format!("OrderBookSnapshots|{interval_ms}");

        Self {
            timer_name: Ustr::from(&timer_name),
            interval_ms,
            snapshot_infos,
            cache,
        }
    }

    /// Publishes a snapshot for each subscribed book.
    ///
    /// Books are cloned out of the cache inside a scoped borrow before publishing,
    /// so subscribers can mutably borrow the cache (e.g. a strategy submitting an
    /// order from `on_book`).
    pub fn snapshot(&self, _event: TimeEvent) {
        let snapshot_infos: Vec<BookSnapshotInfo> =
            self.snapshot_infos.borrow().values().cloned().collect();

        log::debug!(
            "BookSnapshotter.snapshot called for {} subscriptions at {}ms",
            snapshot_infos.len(),
            self.interval_ms,
        );

        let books: Vec<(MStr<Topic>, OrderBook)> = {
            let cache = self.cache.borrow();
            let mut books = Vec::new();

            for snap_info in &snapshot_infos {
                self.collect_snapshot(snap_info, &cache, &mut books);
            }

            books
        };

        for (topic, book) in books {
            msgbus::publish_book(topic, &book);
        }
    }

    fn collect_snapshot(
        &self,
        snap_info: &BookSnapshotInfo,
        cache: &Cache,
        books: &mut Vec<(MStr<Topic>, OrderBook)>,
    ) {
        if let Some((root, class)) = snap_info.parent {
            let topic = snap_info.topic;
            for instrument in cache.instruments_by_parent(&snap_info.venue, &root, class) {
                self.collect_order_book(&instrument.id(), topic, cache, books);
            }
        } else {
            self.collect_order_book(&snap_info.instrument_id, snap_info.topic, cache, books);
        }
    }

    fn collect_order_book(
        &self,
        instrument_id: &InstrumentId,
        topic: MStr<Topic>,
        cache: &Cache,
        books: &mut Vec<(MStr<Topic>, OrderBook)>,
    ) {
        let book = match cache.try_order_book(instrument_id) {
            Ok(book) => book,
            Err(e) => {
                log::error!("Cannot publish OrderBook snapshot: {e}");
                return;
            }
        };

        if book.update_count == 0 {
            log::debug!("OrderBook not yet updated for snapshot: {instrument_id}");
            return;
        }
        log::debug!(
            "Publishing OrderBook snapshot for {instrument_id} (update_count={})",
            book.update_count
        );

        books.push((topic, book.clone()));
    }
}

// Resolves parent expansion components for a book subscription command.
//
// Returns Ok(Some((root, class))) when params carries PARAMS_IS_PARENT=true and
// the instrument_id parses as a recognized <root>.<class> shape; Ok(None) for
// concrete (non-parent) subscriptions; Err when the caller asserts a parent
// subscription but the id cannot be parsed, so subscribe entries can reject up
// front before touching state.
fn resolve_parent_components(
    instrument_id: &InstrumentId,
    params: Option<&Params>,
) -> anyhow::Result<Option<(Ustr, InstrumentClass)>> {
    if !is_parent_subscription(params) {
        return Ok(None);
    }

    let Some((root, class)) = instrument_id.parse_parent_components() else {
        anyhow::bail!(
            "Cannot expand parent subscription for {instrument_id}: \
             symbol does not parse as `<root>.<class>` with a recognized class suffix"
        );
    };

    Ok(Some((Ustr::from(root), class)))
}

#[cfg(test)]
mod tests {
    use nautilus_common::msgbus::TypedHandler;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::BookOrder,
        enums::{BookType, OrderSide},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn snapshot_skips_missing_order_book() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let interval_ms = NonZeroUsize::new(100).unwrap();
        let topic = switchboard::get_book_snapshots_topic(instrument_id, interval_ms);
        let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));

        snapshot_infos.borrow_mut().insert(
            instrument_id,
            BookSnapshotInfo {
                instrument_id,
                venue: Venue::new("SIM"),
                parent: None,
                topic,
                interval_ms,
            },
        );

        let snapshotter = BookSnapshotter::new(
            interval_ms,
            snapshot_infos,
            Rc::new(RefCell::new(Cache::default())),
        );
        let event = TimeEvent::new(
            Ustr::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        snapshotter.snapshot(event);
    }

    #[rstest]
    fn snapshot_allows_subscriber_to_mutably_borrow_cache() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let interval_ms = NonZeroUsize::new(100).unwrap();
        let topic = switchboard::get_book_snapshots_topic(instrument_id, interval_ms);
        let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));

        snapshot_infos.borrow_mut().insert(
            instrument_id,
            BookSnapshotInfo {
                instrument_id,
                venue: Venue::new("SIM"),
                parent: None,
                topic,
                interval_ms,
            },
        );

        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        book.add(
            BookOrder::new(OrderSide::Buy, Price::from("100.00"), Quantity::from(10), 0),
            0,
            1,
            UnixNanos::default(),
        );
        cache.borrow_mut().add_order_book(book).unwrap();

        let received = Rc::new(RefCell::new(Vec::new()));
        let handler = CacheWritingBookHandler {
            id: Ustr::from("CacheWritingBookHandler"),
            cache: cache.clone(),
            received: received.clone(),
        };
        msgbus::subscribe_book_snapshots(topic.into(), TypedHandler::new(handler), None);

        let snapshotter = BookSnapshotter::new(interval_ms, snapshot_infos, cache);
        let event = TimeEvent::new(
            Ustr::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        snapshotter.snapshot(event);

        let received = received.borrow();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].instrument_id, instrument_id);
        assert_eq!(received[0].best_bid_price(), Some(Price::from("100.00")));
    }

    #[rstest]
    fn snapshot_skips_book_with_no_updates() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let interval_ms = NonZeroUsize::new(100).unwrap();
        let topic = switchboard::get_book_snapshots_topic(instrument_id, interval_ms);
        let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));

        snapshot_infos.borrow_mut().insert(
            instrument_id,
            BookSnapshotInfo {
                instrument_id,
                venue: Venue::new("SIM"),
                parent: None,
                topic,
                interval_ms,
            },
        );

        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_order_book(OrderBook::new(instrument_id, BookType::L2_MBP))
            .unwrap();

        let received = Rc::new(RefCell::new(Vec::new()));
        let handler = CacheWritingBookHandler {
            id: Ustr::from("CacheWritingBookHandler-NoUpdates"),
            cache: cache.clone(),
            received: received.clone(),
        };
        msgbus::subscribe_book_snapshots(topic.into(), TypedHandler::new(handler), None);

        let snapshotter = BookSnapshotter::new(interval_ms, snapshot_infos, cache);
        let event = TimeEvent::new(
            Ustr::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        snapshotter.snapshot(event);

        assert!(received.borrow().is_empty());
    }

    struct CacheWritingBookHandler {
        id: Ustr,
        cache: Rc<RefCell<Cache>>,
        received: Rc<RefCell<Vec<OrderBook>>>,
    }

    impl Handler<OrderBook> for CacheWritingBookHandler {
        fn id(&self) -> Ustr {
            self.id
        }

        fn handle(&self, book: &OrderBook) {
            // Mirrors a strategy writing to the cache from `on_book`
            let mut cache = self.cache.borrow_mut();
            let _ = cache.order_book_mut(&book.instrument_id);
            self.received.borrow_mut().push(book.clone());
        }
    }
}
