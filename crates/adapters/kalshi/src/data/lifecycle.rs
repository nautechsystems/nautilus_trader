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
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use anyhow::Context;
use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::{Params, UnixNanos, datetime::try_datetime_to_unix_nanos};
use nautilus_model::{
    data::{Data, InstrumentClose, InstrumentStatus},
    enums::MarketStatusAction,
    identifiers::InstrumentId,
    instruments::{BinaryOption, InstrumentAny},
};
use serde_json::{Map, Value};
use ustr::Ustr;

use crate::{
    common::enums::{MarketLifecycleEvent, MarketStatus},
    http::{
        models::{DecimalNumber, Event, Field, Market},
        parse::{parse_instrument, parse_instrument_close},
    },
    websocket::messages::KalshiDataMessage,
};

#[derive(Debug, Default)]
pub(super) struct LifecycleTracker {
    state: Mutex<State>,
}

impl LifecycleTracker {
    pub(super) fn begin_recovery(&self) {
        self.state
            .lock()
            .expect("lifecycle lock is not poisoned")
            .epoch += 1;
    }

    pub(super) fn begin_read(&self) -> ReadStamp {
        let mut state = self.state.lock().expect("lifecycle lock is not poisoned");
        state.serial += 1;

        ReadStamp {
            epoch: state.epoch,
            serial: state.serial,
        }
    }

    pub(super) fn clear(&self) {
        let mut state = self.state.lock().expect("lifecycle lock is not poisoned");
        state.epoch += 1;
        state.entries.clear();
    }

    pub(super) fn subscribe(
        &self,
        id: InstrumentId,
        close: bool,
        subscribe: bool,
        context: &Mutex<HashMap<InstrumentId, BinaryOption>>,
        sender: &EventSender<DataEvent>,
        now: UnixNanos,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock().expect("lifecycle lock is not poisoned");
        let entry = state.entries.entry(id).or_default();
        let count = if close {
            &mut entry.close_demand
        } else {
            &mut entry.status_demand
        };

        if subscribe {
            *count = count
                .checked_add(1)
                .context("subscription reference count overflow")?;
        } else {
            *count = count.saturating_sub(1);
        }

        if *count == 0 {
            if close {
                entry.close_sent = false;
            } else {
                entry.status_sent = None;
            }
        }

        let context = context.lock().expect("context lock is not poisoned");
        if let Some(instrument) = context.get(&id) {
            let (market, _) = metadata(instrument)?;
            entry.emit(&market, id, instrument.ts_event, now, sender)?;
        }

        Ok(())
    }

    pub(super) fn apply(
        &self,
        mut instrument: BinaryOption,
        stamp: ReadStamp,
        context: &Mutex<HashMap<InstrumentId, BinaryOption>>,
        sender: &EventSender<DataEvent>,
    ) -> anyhow::Result<()> {
        let (mut market, event) = metadata(&instrument)?;

        let source_time = match market.updated_time {
            Field::Value(time) => Some(try_datetime_to_unix_nanos(time)?),
            _ => None,
        };

        let final_close = parse_instrument_close(&market, &event, instrument.ts_init)?;
        let mut state = self.state.lock().expect("lifecycle lock is not poisoned");
        if stamp.epoch != state.epoch {
            return Ok(());
        }

        let entry = state.entries.entry(instrument.id).or_default();

        if !entry.accepts(&instrument, source_time, final_close, stamp.serial)? {
            anyhow::ensure!(
                context
                    .lock()
                    .expect("context lock is not poisoned")
                    .contains_key(&instrument.id),
                "fresh instrument metadata is unavailable during recovery"
            );
            return Ok(());
        }

        let mut value = serde_json::to_value(&market)?;

        for (field, (timestamp, update)) in &entry.overlay {
            if field == "status" && final_close.is_some() {
                continue;
            }

            if source_time.is_none_or(|time| time <= *timestamp) {
                anyhow::ensure!(
                    source_time.is_some() || value[field] == *update,
                    "cannot order contradictory unversioned REST metadata after lifecycle evidence"
                );
                value[field] = update.clone();
            }
        }

        market = serde_json::from_value(value)?;
        instrument = rebuild(&instrument, &market, &event)?;
        for (timestamp, _) in entry.overlay.values() {
            instrument.ts_event = instrument.ts_event.max(*timestamp);
        }

        entry.publish(instrument, &market, &event, context, sender)?;
        entry.serial = stamp.serial;
        entry.version = source_time.or(entry.version);
        entry.overlay.retain(|field, (timestamp, _)| {
            !(field == "status" && final_close.is_some())
                && source_time.is_none_or(|time| time <= *timestamp)
        });

        Ok(())
    }

    pub(super) fn response_guard<'a>(
        &'a self,
        instruments: &[&BinaryOption],
    ) -> anyhow::Result<MutexGuard<'a, State>> {
        let state = self.state.lock().expect("lifecycle lock is not poisoned");
        for instrument in instruments {
            let current = state
                .entries
                .get(&instrument.id)
                .and_then(|entry| entry.definition.as_ref());
            anyhow::ensure!(
                current == Some(&definition(instrument)?),
                "REST instrument response contradicts newer lifecycle metadata"
            );
        }

        Ok(state)
    }

    pub(super) fn observe(
        &self,
        id: InstrumentId,
        data: &KalshiDataMessage,
        context: &Mutex<HashMap<InstrumentId, BinaryOption>>,
        sender: &EventSender<DataEvent>,
        now: UnixNanos,
    ) -> anyhow::Result<()> {
        let Some(millis) = data.sending_ts_ms else {
            return Ok(());
        };

        let timestamp = millis
            .checked_mul(1_000_000)
            .context("lifecycle timestamp overflow")?;
        let timestamp = UnixNanos::from(timestamp);
        let mut state = self.state.lock().expect("lifecycle lock is not poisoned");
        let entry = state.entries.entry(id).or_default();
        if entry.close.is_some()
            || entry.version.is_some_and(|time| timestamp < time)
            || entry.overlay.values().any(|(time, _)| timestamp < *time)
        {
            return Ok(());
        }

        let mut context = context.lock().expect("context lock is not poisoned");

        let Some(instrument) = context.get(&id).cloned() else {
            return Ok(());
        };

        let (mut market, event) = metadata(&instrument)?;
        let mut patch = Map::new();
        let kind = data.msg["event_type"]
            .as_str()
            .context("lifecycle event type is missing")?;

        let status = status_update(kind, data.msg.get("is_deactivated"), &market.status);

        if let Some(status) = status {
            patch.insert("status".into(), serde_json::to_value(status)?);
        }

        for (wire, field) in [("open_ts", "open_time"), ("close_ts", "close_time")] {
            if let Some(value) = data.msg.get(wire) {
                let seconds = value.as_i64().context("invalid lifecycle time")?;
                patch.insert(
                    field.into(),
                    serde_json::to_value(jiff::Timestamp::from_second(seconds)?)?,
                );
            }
        }

        for field in [
            "price_ranges",
            "price_level_structure",
            "yes_sub_title",
            "result",
        ] {
            if let Some(value) = data.msg.get(field) {
                patch.insert(field.into(), value.clone());
            }
        }

        let mut value = serde_json::to_value(&market)?;
        for (field, update) in &patch {
            value[field] = update.clone();
        }

        market = serde_json::from_value(value)?;
        let mut instrument = rebuild(&instrument, &market, &event)?;
        instrument.ts_event = timestamp;
        instrument.ts_init = now;
        entry.commit(instrument, &market, &event, &mut context, sender)?;
        for (field, value) in patch {
            entry.overlay.insert(field, (timestamp, value));
        }

        Ok(())
    }

    pub(super) fn tick(
        &self,
        context: &Mutex<HashMap<InstrumentId, BinaryOption>>,
        sender: &EventSender<DataEvent>,
        now: UnixNanos,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock().expect("lifecycle lock is not poisoned");
        let context = context.lock().expect("context lock is not poisoned");

        for (id, entry) in &mut state.entries {
            if (entry.status_demand > 0 || entry.close_demand > 0)
                && let Some(instrument) = context.get(id)
            {
                let (market, _) = metadata(instrument)?;
                entry.emit(&market, *id, instrument.ts_event, now, sender)?;
            }
        }

        Ok(())
    }
}

fn status_update(
    kind: &str,
    paused: Option<&Value>,
    current: &MarketStatus,
) -> Option<MarketStatus> {
    match (kind.parse::<MarketLifecycleEvent>().ok(), paused, current) {
        (Some(MarketLifecycleEvent::Created), _, _) => Some(MarketStatus::Initialized),
        (Some(MarketLifecycleEvent::Activated), _, _) => Some(MarketStatus::Active),
        (
            Some(MarketLifecycleEvent::Deactivated),
            Some(Value::Bool(paused)),
            MarketStatus::Active | MarketStatus::Inactive,
        ) => Some(if *paused {
            MarketStatus::Inactive
        } else {
            MarketStatus::Active
        }),
        (Some(MarketLifecycleEvent::Deactivated), Some(_), _) => None,
        (Some(MarketLifecycleEvent::Deactivated), None, _) => Some(MarketStatus::Inactive),
        (Some(MarketLifecycleEvent::Determined), _, _) => Some(MarketStatus::Determined),
        (
            Some(MarketLifecycleEvent::Settled),
            _,
            status @ (MarketStatus::Determined | MarketStatus::Disputed | MarketStatus::Amended),
        ) => Some(status.clone()),
        (Some(MarketLifecycleEvent::Settled), _, _) => Some(MarketStatus::Closed),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReadStamp {
    epoch: u64,
    serial: u64,
}

#[derive(Debug, Default)]
pub(super) struct State {
    epoch: u64,
    serial: u64,
    entries: HashMap<InstrumentId, Entry>,
}

#[derive(Debug, Default)]
struct Entry {
    serial: u64,
    version: Option<UnixNanos>,
    overlay: HashMap<String, (UnixNanos, Value)>,
    definition: Option<Value>,
    close: Option<InstrumentClose>,
    status_demand: usize,
    close_demand: usize,
    status_sent: Option<(MarketStatusAction, String)>,
    close_sent: bool,
}

impl Entry {
    fn accepts(
        &self,
        instrument: &BinaryOption,
        source_time: Option<UnixNanos>,
        final_close: Option<InstrumentClose>,
        serial: u64,
    ) -> anyhow::Result<bool> {
        if source_time.is_none()
            && self.version.is_some()
            && (final_close.is_none() || self.close.is_some())
            && let Some(previous) = &self.definition
        {
            anyhow::ensure!(
                definition(instrument)? == *previous,
                "cannot order contradictory unversioned REST metadata after lifecycle evidence"
            );
        }

        if !self.accepts_revisions(instrument)? {
            return Ok(false);
        }

        if let Some(previous) = self.close {
            if let Some(current) = final_close {
                anyhow::ensure!(
                    same_close(previous, current),
                    "conflicting final settlement evidence"
                );
            } else {
                return Ok(false);
            }
        }

        Ok(!(source_time
            .zip(self.version)
            .is_some_and(|(incoming, previous)| incoming < previous)
            || serial < self.serial))
    }

    fn accepts_revisions(&self, instrument: &BinaryOption) -> anyhow::Result<bool> {
        if let Some(previous) = &self.definition {
            for record in ["event", "series"] {
                let previous = previous["info"][record]["last_updated_ts"].as_str();
                let incoming = instrument
                    .info
                    .as_ref()
                    .and_then(|info| info.get(record))
                    .and_then(|value| value.get("last_updated_ts"))
                    .and_then(Value::as_str);

                if let Some(previous) = previous {
                    let incoming = incoming.with_context(|| {
                        format!(
                            "{record} revision is unavailable after versioned {record} metadata"
                        )
                    })?;

                    if incoming.parse::<jiff::Timestamp>()? < previous.parse::<jiff::Timestamp>()? {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }

    fn publish(
        &mut self,
        instrument: BinaryOption,
        market: &Market,
        event: &Event,
        context: &Mutex<HashMap<InstrumentId, BinaryOption>>,
        sender: &EventSender<DataEvent>,
    ) -> anyhow::Result<()> {
        let mut context = context.lock().expect("context lock is not poisoned");
        self.commit(instrument, market, event, &mut context, sender)
    }

    fn commit(
        &mut self,
        instrument: BinaryOption,
        market: &Market,
        event: &Event,
        context: &mut HashMap<InstrumentId, BinaryOption>,
        sender: &EventSender<DataEvent>,
    ) -> anyhow::Result<()> {
        let close = parse_instrument_close(market, event, instrument.ts_init)?;
        if let (Some(previous), Some(current)) = (self.close, close) {
            anyhow::ensure!(
                same_close(previous, current),
                "conflicting final settlement evidence"
            );
        }

        let definition = definition(&instrument)?;
        if self.definition.as_ref() != Some(&definition) || !context.contains_key(&instrument.id) {
            let data = InstrumentAny::BinaryOption(instrument.clone()).into();
            sender
                .send(DataEvent::Data(data))
                .map_err(|_| anyhow::anyhow!("instrument event receiver closed"))?;
        }

        self.definition = Some(definition);
        self.close = close.or(self.close);
        self.emit(
            market,
            instrument.id,
            instrument.ts_event,
            instrument.ts_init,
            sender,
        )?;
        context.insert(instrument.id, instrument);
        Ok(())
    }

    fn emit(
        &mut self,
        market: &Market,
        id: InstrumentId,
        timestamp: UnixNanos,
        now: UnixNanos,
        sender: &EventSender<DataEvent>,
    ) -> anyhow::Result<()> {
        let status = status(market, id, timestamp, now)?;

        let key = (
            status.action,
            status
                .reason
                .map_or_else(String::new, |reason| reason.to_string()),
        );

        if self.status_demand > 0 && self.status_sent.as_ref() != Some(&key) {
            sender
                .send(DataEvent::Data(Data::InstrumentStatus(status)))
                .map_err(|_| anyhow::anyhow!("status event receiver closed"))?;
            self.status_sent = Some(key);
        }

        if self.close_demand > 0
            && !self.close_sent
            && let Some(mut close) = self.close
        {
            close.ts_init = now;
            self.close_sent = crate::execution::settlement::publish_close(sender, close)?;
        }

        Ok(())
    }
}

fn metadata(instrument: &BinaryOption) -> anyhow::Result<(Market, Event)> {
    let info = instrument
        .info
        .as_ref()
        .context("missing instrument metadata")?;
    let market = serde_json::from_value(
        info.get("market")
            .context("missing market metadata")?
            .clone(),
    )?;
    let mut event_info = info.get("event").context("missing event metadata")?.clone();
    let multiplier = event_info
        .get("fee_multiplier_override")
        .and_then(Value::as_str)
        .map(nautilus_core::serialization::decimal::parse)
        .transpose()?;

    if multiplier.is_some() {
        event_info["fee_multiplier_override"] = Value::Null;
    }

    let mut event: Event = serde_json::from_value(event_info)?;
    if let Some(multiplier) = multiplier {
        event.fee_multiplier_override = Field::Value(DecimalNumber(multiplier));
    }

    Ok((market, event))
}

fn rebuild(
    instrument: &BinaryOption,
    market: &Market,
    event: &Event,
) -> anyhow::Result<BinaryOption> {
    let mut updated = parse_instrument(market, event, instrument.ts_init)?;
    if let Some(series) = instrument.info.as_ref().and_then(|info| info.get("series")) {
        let mut info = updated
            .info
            .as_deref()
            .cloned()
            .expect("parsed instrument has metadata");
        info.insert("series".into(), series.clone());
        updated.info = Some(Params::new(info));
    }

    Ok(updated)
}

fn definition(instrument: &BinaryOption) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(instrument)?;
    let object = value
        .as_object_mut()
        .context("invalid serialized instrument")?;
    object.remove("ts_event");
    object.remove("ts_init");

    if let Some(market) = value["info"]["market"].as_object_mut() {
        for field in [
            "updated_time",
            "yes_bid_dollars",
            "yes_bid_size_fp",
            "yes_ask_dollars",
            "yes_ask_size_fp",
            "no_bid_dollars",
            "no_ask_dollars",
            "last_price_dollars",
            "volume_fp",
            "volume_24h_fp",
            "open_interest_fp",
            "previous_yes_bid_dollars",
            "previous_yes_ask_dollars",
            "previous_price_dollars",
        ] {
            market.remove(field);
        }
    }

    Ok(value)
}

fn same_close(left: InstrumentClose, right: InstrumentClose) -> bool {
    left.instrument_id == right.instrument_id
        && left.close_price == right.close_price
        && left.close_type == right.close_type
        && left.ts_event == right.ts_event
}

fn status(
    market: &Market,
    id: InstrumentId,
    mut timestamp: UnixNanos,
    now: UnixNanos,
) -> anyhow::Result<InstrumentStatus> {
    let open = try_datetime_to_unix_nanos(market.open_time)?;
    let close = try_datetime_to_unix_nanos(market.close_time)?;
    let mut reason = &market.status;

    let action = match reason {
        MarketStatus::Initialized | MarketStatus::Active | MarketStatus::Inactive
            if now >= close =>
        {
            timestamp = close;
            reason = &MarketStatus::Closed;
            MarketStatusAction::Close
        }
        MarketStatus::Initialized if now >= open => {
            timestamp = open;
            reason = &MarketStatus::Active;
            MarketStatusAction::Trading
        }
        MarketStatus::Initialized => MarketStatusAction::PreOpen,
        MarketStatus::Active => MarketStatusAction::Trading,
        MarketStatus::Inactive => MarketStatusAction::Pause,
        MarketStatus::Closed => MarketStatusAction::Close,
        MarketStatus::Determined | MarketStatus::Disputed | MarketStatus::Amended => {
            MarketStatusAction::PostClose
        }
        MarketStatus::Finalized => MarketStatusAction::NotAvailableForTrading,
        MarketStatus::Other(_) => anyhow::bail!("unknown market lifecycle status"),
    };

    let trading = action == MarketStatusAction::Trading;
    Ok(InstrumentStatus::new(
        id,
        action,
        timestamp,
        now,
        Some(Ustr::from(reason.as_ref())),
        None,
        Some(trading),
        Some(trading),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::Data;
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::http::models::PriceRange;

    fn source(name: &str) -> (Market, Event) {
        let value: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();

        (
            serde_json::from_value(value["definitions"][name]["market"].clone()).unwrap(),
            serde_json::from_value(value["definitions"][name]["event"].clone()).unwrap(),
        )
    }

    fn frames() -> Vec<KalshiDataMessage> {
        let value: Value =
            serde_json::from_str(include_str!("../../test_data/lifecycle.json")).unwrap();
        serde_json::from_value(value["frames"].clone()).unwrap()
    }

    fn nanos(time: jiff::Timestamp) -> UnixNanos {
        try_datetime_to_unix_nanos(time).unwrap()
    }

    #[rstest]
    fn stream_grid_pause_and_subtitle_survive_older_rest_and_reject_unversioned_conflict() {
        let tracker = LifecycleTracker::default();
        let (market, event) = source("live");
        let now = nanos(market.open_time);
        let instrument = parse_instrument(&market, &event, now).unwrap();
        let id = instrument.id;
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .subscribe(id, false, true, &context, &sender, now)
            .unwrap();
        tracker
            .apply(instrument.clone(), tracker.begin_read(), &context, &sender)
            .unwrap();

        while events.try_recv().is_ok() {}
        let frames = frames();

        for index in [2, 5, 6] {
            tracker
                .observe(id, &frames[index], &context, &sender, now)
                .unwrap();
        }

        tracker
            .apply(instrument, tracker.begin_read(), &context, &sender)
            .unwrap();
        let cached = context.lock().unwrap()[&id].clone();
        let (current, _) = metadata(&cached).unwrap();
        assert_eq!(current.status.as_ref(), "inactive");
        let expected_ranges: Vec<PriceRange> =
            serde_json::from_value(frames[5].msg["price_ranges"].clone()).unwrap();
        assert_ne!(market.price_ranges, expected_ranges);
        assert_eq!(current.price_ranges, expected_ranges);
        assert_eq!(
            cached.price_increment.as_decimal(),
            "0.001".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            cached.description.unwrap().as_str(),
            "Updated exact subtitle"
        );
        tracker.begin_recovery();
        let mut unversioned = market;
        unversioned.updated_time = Field::Missing;
        let unversioned = parse_instrument(&unversioned, &event, now).unwrap();
        assert_eq!(
            tracker
                .apply(unversioned, tracker.begin_read(), &context, &sender)
                .unwrap_err()
                .to_string(),
            "cannot order contradictory unversioned REST metadata after lifecycle evidence"
        );
        let output: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();

        let statuses: Vec<_> = output
            .iter()
            .filter_map(|event| {
                if let DataEvent::Data(Data::InstrumentStatus(status)) = event {
                    Some(status)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(statuses.len(), 1);
        let status = statuses[0];
        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::Pause);
        assert_eq!(status.reason, Some(Ustr::from("inactive")));
        assert_eq!(status.trading_event, None);
        assert_eq!(status.is_trading, Some(false));
        assert_eq!(status.is_quoting, Some(false));
        assert_eq!(status.is_short_sell_restricted, None);
        assert_eq!(
            status.ts_event.as_u64(),
            frames[2].sending_ts_ms.unwrap() * 1_000_000
        );
        assert_eq!(status.ts_init, now);
    }

    #[rstest]
    fn newer_rest_wins_over_queued_stream_and_reconnect_discards_old_reads() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        let now = nanos(market.open_time);
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let old_read = tracker.begin_read();
        tracker.begin_recovery();
        market.updated_time = Field::Value(
            market
                .open_time
                .checked_add(jiff::SignedDuration::from_secs(20))
                .unwrap(),
        );
        market.yes_sub_title = "new REST revision".into();
        let current = parse_instrument(&market, &event, now).unwrap();
        let id = current.id;
        tracker
            .apply(current, tracker.begin_read(), &context, &sender)
            .unwrap();
        tracker
            .observe(id, &frames()[2], &context, &sender, now)
            .unwrap();
        market.yes_sub_title = "obsolete request".into();
        let obsolete = parse_instrument(&market, &event, now).unwrap();
        tracker
            .apply(obsolete, old_read, &context, &sender)
            .unwrap();
        assert_eq!(
            context.lock().unwrap()[&id].description.unwrap().as_str(),
            "new REST revision"
        );
        assert_eq!(
            metadata(&context.lock().unwrap()[&id])
                .unwrap()
                .0
                .status
                .as_ref(),
            "active"
        );
        assert!(matches!(
            events.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));
        assert!(events.try_recv().is_err());
    }

    #[rstest]
    fn implicit_open_close_and_expiry_do_not_create_settlement() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        market.status = "initialized".parse().unwrap();
        let open = nanos(market.open_time);
        let close = nanos(market.close_time);
        let before = UnixNanos::from(open.as_u64() - 1);
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let instrument = parse_instrument(&market, &event, before).unwrap();
        let id = instrument.id;
        tracker
            .subscribe(id, false, true, &context, &sender, before)
            .unwrap();
        tracker
            .subscribe(id, true, true, &context, &sender, before)
            .unwrap();
        tracker
            .apply(instrument, tracker.begin_read(), &context, &sender)
            .unwrap();
        tracker.tick(&context, &sender, open).unwrap();
        tracker.tick(&context, &sender, close).unwrap();
        tracker
            .tick(&context, &sender, nanos(market.latest_expiration_time))
            .unwrap();
        let output: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();

        let statuses: Vec<_> = output
            .iter()
            .filter_map(|event| {
                if let DataEvent::Data(Data::InstrumentStatus(status)) = event {
                    Some(status)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            statuses
                .iter()
                .map(|status| status.action)
                .collect::<Vec<_>>(),
            vec![
                MarketStatusAction::PreOpen,
                MarketStatusAction::Trading,
                MarketStatusAction::Close
            ]
        );
        assert_eq!(statuses[1].ts_event, open);
        assert_eq!(statuses[2].ts_event, close);
        assert_eq!(output.len(), 4);
    }

    #[rstest]
    fn final_evidence_emits_once_survives_reconnect_and_rejects_conflicts() {
        let tracker = LifecycleTracker::default();
        let (market, event) = source("settled");
        let now = nanos(market.latest_expiration_time);
        let instrument = parse_instrument(&market, &event, now).unwrap();
        let id = instrument.id;
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .subscribe(id, true, true, &context, &sender, now)
            .unwrap();
        tracker
            .apply(instrument.clone(), tracker.begin_read(), &context, &sender)
            .unwrap();
        tracker.begin_recovery();
        tracker
            .apply(instrument.clone(), tracker.begin_read(), &context, &sender)
            .unwrap();
        let mut nonfinal = market.clone();
        nonfinal.status = "determined".parse().unwrap();
        tracker
            .apply(
                parse_instrument(&nonfinal, &event, now).unwrap(),
                tracker.begin_read(),
                &context,
                &sender,
            )
            .unwrap();
        let mut conflict = market.clone();
        conflict.settlement_value_dollars =
            Field::Value(crate::http::models::FixedPoint(Decimal::ZERO));
        assert_eq!(
            tracker
                .apply(
                    parse_instrument(&conflict, &event, now).unwrap(),
                    tracker.begin_read(),
                    &context,
                    &sender
                )
                .unwrap_err()
                .to_string(),
            "conflicting final settlement evidence"
        );
        let output: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert_eq!(output.len(), 2);

        let DataEvent::Data(Data::InstrumentClose(close)) = &output[1] else {
            panic!()
        };

        assert_eq!(
            *close,
            parse_instrument_close(&market, &event, now)
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            metadata(&context.lock().unwrap()[&id])
                .unwrap()
                .0
                .status
                .as_ref(),
            "finalized"
        );
        let restarted = LifecycleTracker::default();
        let restored = Mutex::new(HashMap::new());
        restarted
            .subscribe(id, true, true, &restored, &sender, now)
            .unwrap();
        restarted
            .apply(instrument, restarted.begin_read(), &restored, &sender)
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));

        let DataEvent::Data(Data::InstrumentClose(recovered)) = events.try_recv().unwrap() else {
            panic!()
        };

        assert_eq!(recovered, *close);
    }

    #[rstest]
    fn lifecycle_transcript_preserves_every_transition_and_changes_definitions_only_when_needed() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        market.status = "initialized".parse().unwrap();
        market.open_time = market
            .open_time
            .checked_add(jiff::SignedDuration::from_secs(2))
            .unwrap();
        let initial = nanos(market.created_time);
        let instrument = parse_instrument(&market, &event, initial).unwrap();
        let id = instrument.id;
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .subscribe(id, false, true, &context, &sender, initial)
            .unwrap();
        tracker
            .subscribe(id, true, true, &context, &sender, initial)
            .unwrap();
        tracker
            .apply(instrument, tracker.begin_read(), &context, &sender)
            .unwrap();

        while events.try_recv().is_ok() {}
        let transcript = frames();
        for frame in &transcript {
            let now = UnixNanos::from(frame.sending_ts_ms.unwrap() * 1_000_000);
            tracker.observe(id, frame, &context, &sender, now).unwrap();
            tracker.observe(id, frame, &context, &sender, now).unwrap();
        }

        tracker
            .observe(
                id,
                &transcript[3],
                &context,
                &sender,
                UnixNanos::from(transcript[8].sending_ts_ms.unwrap() * 1_000_000),
            )
            .unwrap();

        let output: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();

        let statuses: Vec<_> = output
            .iter()
            .filter_map(|event| {
                if let DataEvent::Data(Data::InstrumentStatus(status)) = event {
                    Some(status)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            statuses
                .iter()
                .map(|status| (status.action, status.reason.unwrap().to_string()))
                .collect::<Vec<_>>(),
            vec![
                (MarketStatusAction::Trading, "active".into()),
                (MarketStatusAction::Pause, "inactive".into()),
                (MarketStatusAction::Trading, "active".into()),
                (MarketStatusAction::Close, "closed".into()),
                (MarketStatusAction::PostClose, "determined".into())
            ]
        );
        assert!(
            output
                .iter()
                .all(|event| !matches!(event, DataEvent::Data(Data::InstrumentClose(_))))
        );
        let current = context.lock().unwrap()[&id].clone();
        assert_eq!(current.price_precision, 6);
        assert_eq!(current.size_precision, 2);
        assert_eq!(
            current.description.unwrap().as_str(),
            "Updated exact subtitle"
        );
        assert_eq!(
            metadata(&current).unwrap().0.close_time.as_second(),
            transcript[4].msg["close_ts"].as_i64().unwrap()
        );
        assert_eq!(
            current.price_increment.as_decimal(),
            "0.001".parse::<Decimal>().unwrap()
        );
    }

    #[rstest]
    fn rest_dispute_amendment_expiry_and_grid_refresh_emit_meaningful_changes() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let id = crate::common::parse::parse_instrument_id(&market.ticker).unwrap();
        let now = nanos(market.close_time);
        tracker
            .subscribe(id, false, true, &context, &sender, now)
            .unwrap();

        for (index, status) in ["determined", "disputed", "amended"]
            .into_iter()
            .enumerate()
        {
            market.status = status.parse().unwrap();
            market.updated_time = Field::Value(
                market
                    .close_time
                    .checked_add(jiff::SignedDuration::from_secs(index as i64 + 1))
                    .unwrap(),
            );
            market.latest_expiration_time = market
                .latest_expiration_time
                .checked_add(jiff::SignedDuration::from_secs(1))
                .unwrap();
            market.price_ranges =
                serde_json::from_value(frames()[5].msg["price_ranges"].clone()).unwrap();
            let instrument = parse_instrument(&market, &event, now).unwrap();
            tracker
                .apply(instrument.clone(), tracker.begin_read(), &context, &sender)
                .unwrap();
            tracker
                .apply(instrument, tracker.begin_read(), &context, &sender)
                .unwrap();
        }

        let output: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert_eq!(output.len(), 6);

        for (index, status) in ["determined", "disputed", "amended"]
            .into_iter()
            .enumerate()
        {
            let DataEvent::Data(Data::Instrument(instrument)) = &output[index * 2] else {
                panic!()
            };

            let InstrumentAny::BinaryOption(instrument) = instrument.as_ref() else {
                panic!()
            };

            assert_eq!(instrument.id, id);
            assert_eq!(
                instrument.info.as_ref().unwrap()["market"]["price_ranges"],
                frames()[5].msg["price_ranges"]
            );
            assert_eq!(
                instrument.price_increment.as_decimal(),
                "0.001".parse::<Decimal>().unwrap()
            );

            let DataEvent::Data(Data::InstrumentStatus(value)) = &output[index * 2 + 1] else {
                panic!()
            };

            assert_eq!(value.action, MarketStatusAction::PostClose);
            assert_eq!(value.reason.unwrap().as_str(), status);
            assert_eq!(
                value.ts_event,
                UnixNanos::from(now.as_u64() + (index as u64 + 1) * 1_000_000_000)
            );
            assert_eq!(value.ts_init, now);
        }

        assert_eq!(
            context.lock().unwrap()[&id].expiration_ns,
            nanos(market.latest_expiration_time)
        );
    }

    #[rstest]
    fn status_and_close_demand_have_independent_reference_counts() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        let now = nanos(market.open_time);
        let id = crate::common::parse::parse_instrument_id(&market.ticker).unwrap();
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .subscribe(id, false, true, &context, &sender, now)
            .unwrap();
        tracker
            .subscribe(id, false, true, &context, &sender, now)
            .unwrap();
        tracker
            .subscribe(id, true, true, &context, &sender, now)
            .unwrap();
        tracker
            .apply(
                parse_instrument(&market, &event, now).unwrap(),
                tracker.begin_read(),
                &context,
                &sender,
            )
            .unwrap();

        while events.try_recv().is_ok() {}
        tracker
            .subscribe(id, false, false, &context, &sender, now)
            .unwrap();
        tracker
            .subscribe(id, true, false, &context, &sender, now)
            .unwrap();
        tracker
            .tick(&context, &sender, nanos(market.close_time))
            .unwrap();

        let DataEvent::Data(Data::InstrumentStatus(status)) = events.try_recv().unwrap() else {
            panic!()
        };

        assert_eq!(status.action, MarketStatusAction::Close);
        assert_eq!(status.instrument_id, id);
        tracker
            .subscribe(
                id,
                false,
                false,
                &context,
                &sender,
                nanos(market.close_time),
            )
            .unwrap();
        market.status = "finalized".parse().unwrap();
        market.settlement_ts = Field::Value(market.close_time);
        market.settlement_value_dollars =
            Field::Value(crate::http::models::FixedPoint("0.33".parse().unwrap()));
        tracker
            .apply(
                parse_instrument(&market, &event, now).unwrap(),
                tracker.begin_read(),
                &context,
                &sender,
            )
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));
        assert!(events.try_recv().is_err());
        tracker
            .subscribe(id, true, true, &context, &sender, now)
            .unwrap();
        tracker
            .subscribe(id, true, true, &context, &sender, now)
            .unwrap();
        tracker
            .subscribe(id, true, false, &context, &sender, now)
            .unwrap();
        tracker.tick(&context, &sender, now).unwrap();

        let DataEvent::Data(Data::InstrumentClose(close)) = events.try_recv().unwrap() else {
            panic!()
        };

        assert_eq!(
            close,
            parse_instrument_close(&market, &event, now)
                .unwrap()
                .unwrap()
        );
        assert!(events.try_recv().is_err());
    }

    #[rstest]
    fn notification_without_venue_time_requires_fresh_rest_instead_of_overwriting_state() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        let now = nanos(market.open_time);
        let id = crate::common::parse::parse_instrument_id(&market.ticker).unwrap();
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .subscribe(id, false, true, &context, &sender, now)
            .unwrap();
        tracker
            .apply(
                parse_instrument(&market, &event, now).unwrap(),
                tracker.begin_read(),
                &context,
                &sender,
            )
            .unwrap();

        while events.try_recv().is_ok() {}
        let mut frame = frames()[2].clone();
        frame.sending_ts_ms = None;
        tracker
            .observe(
                id,
                &frame,
                &context,
                &sender,
                UnixNanos::from(now.as_u64() + 10_000_000_000),
            )
            .unwrap();
        assert_eq!(
            metadata(&context.lock().unwrap()[&id])
                .unwrap()
                .0
                .status
                .as_ref(),
            "active"
        );
        assert!(events.try_recv().is_err());
        market.status = "inactive".parse().unwrap();
        market.updated_time = Field::Value(
            market
                .open_time
                .checked_add(jiff::SignedDuration::from_secs(3))
                .unwrap(),
        );
        tracker
            .apply(
                parse_instrument(&market, &event, now).unwrap(),
                tracker.begin_read(),
                &context,
                &sender,
            )
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));

        let DataEvent::Data(Data::InstrumentStatus(status)) = events.try_recv().unwrap() else {
            panic!()
        };

        assert_eq!(status.action, MarketStatusAction::Pause);
        assert_eq!(status.reason, Some(Ustr::from("inactive")));
        assert_eq!(status.ts_event.as_u64(), now.as_u64() + 3_000_000_000);
        assert!(events.try_recv().is_err());
    }

    #[rstest]
    fn correlated_responses_cannot_overwrite_newer_stream_metadata() {
        let tracker = LifecycleTracker::default();
        let (market, event) = source("live");
        let now = nanos(market.open_time);
        let instrument = parse_instrument(&market, &event, now).unwrap();
        let id = instrument.id;
        let context = Mutex::new(HashMap::new());
        let (sender, _events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .apply(instrument.clone(), tracker.begin_read(), &context, &sender)
            .unwrap();
        tracker
            .observe(id, &frames()[5], &context, &sender, now)
            .unwrap();
        assert_eq!(
            tracker
                .response_guard(&[&instrument])
                .unwrap_err()
                .to_string(),
            "REST instrument response contradicts newer lifecycle metadata"
        );
        let latest = context.lock().unwrap()[&id].clone();
        assert!(tracker.response_guard(&[&latest]).is_ok());
        assert_eq!(
            latest.price_increment.as_decimal(),
            "0.001".parse::<Decimal>().unwrap()
        );
    }

    #[rstest]
    #[case::event("event")]
    #[case::series("series")]
    fn older_metadata_revision_cannot_replace_newer_metadata(#[case] record: &str) {
        let tracker = LifecycleTracker::default();
        let (market, event) = source("live");
        let now = nanos(market.open_time);
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let mut current = parse_instrument(&market, &event, now).unwrap();
        let values: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();

        let mut info = current.info.as_deref().unwrap().clone();
        if record == "series" {
            info.insert(record.into(), values["series"]["series"].clone());
        }

        info[record]["last_updated_ts"] = serde_json::to_value(market.open_time).unwrap();
        info[record]["title"] = "new metadata title".into();
        current.info = Some(Params::new(info.clone()));
        let id = current.id;
        tracker
            .apply(current.clone(), tracker.begin_read(), &context, &sender)
            .unwrap();

        while events.try_recv().is_ok() {}
        let mut older = current.clone();
        let mut info_older = info.clone();
        info_older[record]["last_updated_ts"] = serde_json::to_value(market.created_time).unwrap();
        info_older[record]["title"] = "old metadata title".into();
        older.info = Some(Params::new(info_older));
        tracker
            .apply(older, tracker.begin_read(), &context, &sender)
            .unwrap();
        assert_eq!(
            context.lock().unwrap()[&id].info.as_ref().unwrap()[record]["title"],
            "new metadata title"
        );
        assert!(events.try_recv().is_err());
        info[record]
            .as_object_mut()
            .unwrap()
            .remove("last_updated_ts");
        current.info = Some(Params::new(info));
        let missing = tracker.apply(current, tracker.begin_read(), &context, &sender);
        assert_eq!(
            missing.unwrap_err().to_string(),
            format!("{record} revision is unavailable after versioned {record} metadata")
        );
        assert_eq!(
            context.lock().unwrap()[&id].info.as_ref().unwrap()[record]["title"],
            "new metadata title"
        );
    }

    #[rstest]
    fn repeated_unversioned_final_evidence_cannot_replace_instrument_metadata() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("settled");
        market.updated_time = Field::Value(market.close_time);
        let now = nanos(market.close_time);
        let instrument = parse_instrument(&market, &event, now).unwrap();
        let id = instrument.id;
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .apply(instrument, tracker.begin_read(), &context, &sender)
            .unwrap();

        while events.try_recv().is_ok() {}
        let expected = context.lock().unwrap()[&id].description;
        market.updated_time = Field::Missing;
        market.yes_sub_title = "older unordered subtitle".into();
        let result = tracker.apply(
            parse_instrument(&market, &event, now).unwrap(),
            tracker.begin_read(),
            &context,
            &sender,
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "cannot order contradictory unversioned REST metadata after lifecycle evidence"
        );
        assert_eq!(context.lock().unwrap()[&id].description, expected);
        assert!(events.try_recv().is_err());
    }

    #[rstest]
    fn deactivation_flag_resumes_open_market_and_survives_stale_rest() {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        let now = nanos(market.open_time);
        let instrument = parse_instrument(&market, &event, now).unwrap();
        let id = instrument.id;
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .subscribe(id, false, true, &context, &sender, now)
            .unwrap();
        tracker
            .apply(instrument, tracker.begin_read(), &context, &sender)
            .unwrap();

        while events.try_recv().is_ok() {}
        let frames = frames();
        for frame in [&frames[2], &frames[3]] {
            tracker.observe(id, frame, &context, &sender, now).unwrap();
        }

        market.status = "inactive".parse().unwrap();
        tracker
            .apply(
                parse_instrument(&market, &event, now).unwrap(),
                tracker.begin_read(),
                &context,
                &sender,
            )
            .unwrap();

        let statuses: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| {
                if let DataEvent::Data(Data::InstrumentStatus(status)) = event {
                    Some(status)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            statuses,
            vec![
                InstrumentStatus::new(
                    id,
                    MarketStatusAction::Pause,
                    UnixNanos::from(frames[2].sending_ts_ms.unwrap() * 1_000_000),
                    now,
                    Some(Ustr::from("inactive")),
                    None,
                    Some(false),
                    Some(false),
                    None
                ),
                InstrumentStatus::new(
                    id,
                    MarketStatusAction::Trading,
                    UnixNanos::from(frames[3].sending_ts_ms.unwrap() * 1_000_000),
                    now,
                    Some(Ustr::from("active")),
                    None,
                    Some(true),
                    Some(true),
                    None
                ),
            ]
        );
        assert_eq!(
            metadata(&context.lock().unwrap()[&id])
                .unwrap()
                .0
                .status
                .as_ref(),
            "active"
        );
    }

    #[rstest]
    #[case::initialized("initialized")]
    #[case::closed("closed")]
    #[case::determined("determined")]
    #[case::disputed("disputed")]
    #[case::amended("amended")]
    fn pause_flags_do_not_change_non_open_markets(#[case] initial: &str) {
        let tracker = LifecycleTracker::default();
        let (mut market, event) = source("live");
        market.status = initial.parse().unwrap();
        let now = UnixNanos::from(nanos(market.open_time).as_u64() - 1_000_000_000);
        let instrument = parse_instrument(&market, &event, now).unwrap();
        let id = instrument.id;
        let context = Mutex::new(HashMap::new());
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        tracker
            .subscribe(id, false, true, &context, &sender, now)
            .unwrap();
        tracker
            .apply(instrument, tracker.begin_read(), &context, &sender)
            .unwrap();

        while events.try_recv().is_ok() {}
        let frames = frames();
        for frame in [&frames[2], &frames[3]] {
            tracker.observe(id, frame, &context, &sender, now).unwrap();
        }

        assert!(events.try_recv().is_err());
        assert_eq!(
            metadata(&context.lock().unwrap()[&id])
                .unwrap()
                .0
                .status
                .as_ref(),
            initial
        );

        if initial == "initialized" {
            let open = nanos(market.open_time);
            tracker.tick(&context, &sender, open).unwrap();

            let DataEvent::Data(Data::InstrumentStatus(status)) = events.try_recv().unwrap() else {
                panic!("expected clock-driven open")
            };

            assert_eq!(
                status,
                InstrumentStatus::new(
                    id,
                    MarketStatusAction::Trading,
                    open,
                    open,
                    Some(Ustr::from("active")),
                    None,
                    Some(true),
                    Some(true),
                    None
                )
            );
            assert!(events.try_recv().is_err());
        }
    }

    #[rstest]
    #[case("0.5")]
    #[case("0.1234567890123456789012345678")]
    fn event_fee_override_survives_instrument_metadata_and_rebuild(#[case] value: &str) {
        use crate::http::models::DecimalNumber;
        let (market, mut event) = source("live");
        event.fee_multiplier_override = Field::Value(DecimalNumber(value.parse().unwrap()));
        event.fee_type_override = Field::Value("quadratic".parse().unwrap());
        event.markets = Field::Missing;
        let instrument = parse_instrument(&market, &event, UnixNanos::from(123)).unwrap();
        let (restored_market, restored_event) = metadata(&instrument).unwrap();
        assert_eq!(restored_market, market);
        assert_eq!(restored_event, event);
        let rebuilt = rebuild(&instrument, &restored_market, &restored_event).unwrap();
        assert_eq!(rebuilt.info, instrument.info);
    }
}
