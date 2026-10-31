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

//! Serial polling with endpoint-scoped schedules and publication gates.

use std::{collections::HashMap, fmt::Debug, hash::Hash, sync::Arc, time::Duration};

use jiff::Timestamp;
use nautilus_common::{
    cache::fifo::FifoCache,
    live::{dst::time, sender::EventSender},
    messages::DataEvent,
};
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{BarType, BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDepth, QuoteTick},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    instruments::BinaryOption,
    types::{Price, Quantity},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{Publication, history, parse, send};
use crate::{
    common::symbol::from_instrument_id,
    http::{
        client::FinFeedHttpClient,
        error::Error,
        models::{ActivityBatch, Book, Ohlcv, OhlcvPeriod},
        query::{ActivityQuery, OHLCV_LIMIT_MAX, OhlcvQuery, PAGE_LIMIT_MAX},
    },
};

const SUBSCRIPTIONS_MAX: usize = 128;
const IDENTITIES_MIN: usize = 2048;
const IDENTITIES_MAX: usize = 2 * PAGE_LIMIT_MAX as usize;
const _: () = assert!(PAGE_LIMIT_MAX < OHLCV_LIMIT_MAX);

pub(super) async fn run(
    state: Arc<Mutex<Publication>>,
    sender: EventSender<DataEvent>,
    http: Arc<FinFeedHttpClient>,
    changed: Arc<tokio::sync::Notify>,
    cancellation: CancellationToken,
    interval: Duration,
    limit: u32,
) {
    loop {
        let change = changed.notified();
        let next = state.lock().polling.next();
        let deadline = next.as_ref().map(|(_, _, due)| *due);
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return,
            () = wait(deadline) => {},
            () = change => continue,
        }
        let Some((id, feed, _)) = next else { continue };

        let (revision, instrument) = {
            let state = state.lock();
            if !state.connected {
                return;
            }

            let Some(instrument) = state.instruments.get(&id).cloned() else {
                continue;
            };

            let Some(schedule) = state.polling.schedule(id, feed) else {
                continue;
            };

            (schedule.revision, instrument)
        };

        let (exchange, market) = match from_instrument_id(id) {
            Ok(ids) => ids,
            Err(e) => {
                log::error!("FinFeed poll identity failed: {e}");
                return;
            }
        };

        let query = ActivityQuery { limit };

        let Some(result) = request(
            &state,
            &http,
            &changed,
            &cancellation,
            (id, feed, revision),
            (&exchange, &market),
            &query,
        )
        .await
        else {
            continue;
        };

        let mut state = state.lock();
        if !state.connected {
            return;
        }

        if !state
            .polling
            .schedule(id, feed)
            .is_some_and(|schedule| schedule.revision == revision)
        {
            continue;
        }

        let Some(scope) = state.polling.subscriptions.get_mut(&id) else {
            continue;
        };

        if let Err(e) = scope.complete(feed, result, &instrument, &sender, interval, limit) {
            log::error!("FinFeed poll publication failed: {e}");
            return;
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Polling {
    subscriptions: HashMap<InstrumentId, Scope>,
    revision: u64,
}

impl Polling {
    pub(super) fn subscribe(
        &mut self,
        id: InstrumentId,
        quotes: bool,
        limit: u32,
    ) -> anyhow::Result<bool> {
        let revision = self.revision.wrapping_add(1);
        let scope = self.subscription(id)?;
        let activity = scope.activity.get_or_insert_with(ActivityScope::default);
        if !activity.schedule.suspended
            && if quotes {
                activity.quote_ids.is_some()
            } else {
                activity.trade_ids.is_some()
            }
        {
            return Ok(false);
        }

        if quotes {
            activity
                .quote_ids
                .get_or_insert_with(|| Identities::new(limit));
        } else {
            activity
                .trade_ids
                .get_or_insert_with(|| Identities::new(limit));
        }

        activity.schedule.suspended = false;
        activity.schedule.due = time::Instant::now();
        activity.schedule.revision = revision;
        self.revision = revision;
        Ok(true)
    }

    pub(super) fn unsubscribe(&mut self, id: InstrumentId, quotes: bool) -> bool {
        let Some(scope) = self.subscriptions.get_mut(&id) else {
            return false;
        };

        let Some(activity) = &mut scope.activity else {
            return false;
        };

        if !(if quotes {
            activity.quote_ids.is_some()
        } else {
            activity.trade_ids.is_some()
        }) {
            return false;
        }

        self.revision = self.revision.wrapping_add(1);
        activity.schedule.revision = self.revision;
        activity.schedule.due = time::Instant::now();
        if quotes {
            activity.quote_ids = None;
        } else {
            activity.trade_ids = None;
        }

        if activity.quote_ids.is_none() && activity.trade_ids.is_none() {
            scope.activity = None;
        }

        if scope.is_empty() {
            self.subscriptions.remove(&id);
        }

        true
    }

    pub(super) fn instrument_ids(&self) -> Vec<InstrumentId> {
        let mut ids: Vec<_> = self.subscriptions.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub(super) fn deactivate(&mut self, id: InstrumentId) {
        if self.subscriptions.remove(&id).is_some() {
            log::debug!("Deactivated FinFeed polling subscriptions for {id}");
        }
    }

    pub(super) fn refresh(&mut self, id: InstrumentId) {
        let Some(scope) = self.subscriptions.get_mut(&id) else {
            return;
        };

        self.revision = self.revision.wrapping_add(1);
        let revision = self.revision;
        let schedules = scope
            .activity
            .iter_mut()
            .map(|activity| &mut activity.schedule)
            .chain(scope.book.iter_mut().map(|book| &mut book.schedule))
            .chain(scope.bars.values_mut().map(|bars| &mut bars.schedule));

        for schedule in schedules {
            schedule.revision = revision;
            schedule.due = time::Instant::now();
        }
    }

    pub(super) fn restore(&mut self) {
        self.revision = self.revision.wrapping_add(1);

        for scope in self.subscriptions.values_mut() {
            if let Some(activity) = &mut scope.activity {
                activity.schedule = Schedule {
                    revision: self.revision,
                    ..Default::default()
                };
            }

            if let Some(book) = &mut scope.book {
                book.schedule = Schedule {
                    revision: self.revision,
                    ..Default::default()
                };

                book.previous = None;
                book.published_revision = 0;
            }

            for bars in scope.bars.values_mut() {
                bars.schedule = Schedule {
                    revision: self.revision,
                    ..Default::default()
                };

                bars.period = None;
            }
        }
    }

    pub(super) fn subscribe_book(
        &mut self,
        id: InstrumentId,
        deltas: bool,
        depth: Option<usize>,
    ) -> anyhow::Result<bool> {
        let revision = self.revision.wrapping_add(1);
        let scope = self.subscription(id)?;
        let book = scope.book.get_or_insert_with(BookScope::default);
        let unchanged = if deltas {
            book.deltas && book.deltas_depth == depth
        } else {
            book.depth == depth
        };

        if !book.schedule.suspended && unchanged {
            return Ok(false);
        }

        if deltas {
            book.deltas = true;
            book.deltas_depth = depth;
        } else {
            book.depth = depth;
        }

        book.schedule = Schedule {
            revision,
            ..Default::default()
        };

        self.revision = revision;
        Ok(true)
    }

    pub(super) fn unsubscribe_book(&mut self, id: InstrumentId, deltas: bool) -> bool {
        let Some(scope) = self.subscriptions.get_mut(&id) else {
            return false;
        };

        let Some(book) = &mut scope.book else {
            return false;
        };

        if deltas {
            if !book.deltas {
                return false;
            }

            book.deltas = false;
            book.deltas_depth = None;
        } else if book.depth.take().is_none() {
            return false;
        }

        self.revision = self.revision.wrapping_add(1);
        book.schedule.revision = self.revision;
        book.schedule.due = time::Instant::now();
        if !book.deltas && book.depth.is_none() {
            scope.book = None;
        }

        if scope.is_empty() {
            self.subscriptions.remove(&id);
        }

        true
    }

    pub(super) fn subscribe_bars(
        &mut self,
        bar_type: BarType,
        now: Timestamp,
    ) -> anyhow::Result<bool> {
        let id = bar_type.instrument_id();
        let exists = self
            .subscriptions
            .get(&id)
            .is_some_and(|scope| scope.bars.contains_key(&bar_type));
        anyhow::ensure!(
            exists
                || self
                    .subscriptions
                    .values()
                    .map(|scope| scope.bars.len())
                    .sum::<usize>()
                    < SUBSCRIPTIONS_MAX,
            "FinFeed bar subscription limit exceeded"
        );
        let revision = self.revision.wrapping_add(1);
        let scope = self.subscription(id)?;

        let bars = scope.bars.entry(bar_type).or_insert_with(|| BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: now,
        });

        if exists && !bars.schedule.suspended {
            return Ok(false);
        }

        bars.schedule = Schedule {
            revision,
            ..Default::default()
        };

        self.revision = revision;
        Ok(true)
    }

    pub(super) fn unsubscribe_bars(&mut self, bar_type: BarType) -> bool {
        let id = bar_type.instrument_id();

        let Some(scope) = self.subscriptions.get_mut(&id) else {
            return false;
        };

        if scope.bars.remove(&bar_type).is_none() {
            return false;
        }

        self.revision = self.revision.wrapping_add(1);

        if scope.is_empty() {
            self.subscriptions.remove(&id);
        }

        true
    }

    fn subscription(&mut self, id: InstrumentId) -> anyhow::Result<&mut Scope> {
        anyhow::ensure!(
            self.subscriptions.contains_key(&id) || self.subscriptions.len() < SUBSCRIPTIONS_MAX,
            "FinFeed subscription limit exceeded"
        );
        Ok(self.subscriptions.entry(id).or_default())
    }

    fn schedule(&self, id: InstrumentId, feed: Feed) -> Option<&Schedule> {
        let scope = self.subscriptions.get(&id)?;
        match feed {
            Feed::Activity => scope.activity.as_ref().map(|activity| &activity.schedule),
            Feed::Book => scope.book.as_ref().map(|book| &book.schedule),
            Feed::Bars(bar_type) => scope.bars.get(&bar_type).map(|bars| &bars.schedule),
        }
    }

    fn next(&self) -> Option<(InstrumentId, Feed, time::Instant)> {
        self.subscriptions
            .iter()
            .flat_map(|(id, scope)| {
                [
                    (
                        Feed::Activity,
                        scope.activity.as_ref().map(|activity| &activity.schedule),
                    ),
                    (Feed::Book, scope.book.as_ref().map(|book| &book.schedule)),
                ]
                .into_iter()
                .chain(
                    scope
                        .bars
                        .iter()
                        .map(|(bar_type, bars)| (Feed::Bars(*bar_type), Some(&bars.schedule))),
                )
                .filter_map(move |(feed, schedule)| {
                    schedule
                        .filter(|schedule| !schedule.suspended)
                        .map(|schedule| (*id, feed, schedule.due))
                })
            })
            .min_by_key(|(id, feed, due)| (*due, *id, *feed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Feed {
    Activity,
    Book,
    Bars(BarType),
}

enum Batch {
    Activity(ActivityBatch),
    Book(Book),
    Bars(BarType, OhlcvPeriod, Timestamp, Vec<Ohlcv>),
}

#[derive(Debug)]
struct Schedule {
    revision: u64,
    suspended: bool,
    due: time::Instant,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            revision: 0,
            suspended: false,
            due: time::Instant::now(),
        }
    }
}

#[derive(Debug, Default)]
struct Scope {
    activity: Option<ActivityScope>,
    book: Option<BookScope>,
    bars: HashMap<BarType, BarScope>,
}

#[derive(Debug, Default)]
struct ActivityScope {
    schedule: Schedule,
    quote_ids: Option<Identities<QuoteTick>>,
    trade_ids: Option<Identities<TradeId>>,
}

#[derive(Debug)]
enum Identities<T: Clone + Debug + Eq + Hash> {
    Small(FifoCache<T, IDENTITIES_MIN>),
    Large(FifoCache<T, IDENTITIES_MAX>),
}

impl<T: Clone + Debug + Eq + Hash> Identities<T> {
    fn new(limit: u32) -> Self {
        if 2 * limit as usize <= IDENTITIES_MIN {
            let ids = FifoCache::new();
            Self::Small(ids)
        } else {
            let ids = FifoCache::new();
            Self::Large(ids)
        }
    }

    fn contains(&self, id: &T) -> bool {
        match self {
            Self::Small(ids) => ids.contains(id),
            Self::Large(ids) => ids.contains(id),
        }
    }

    fn add(&mut self, id: T) {
        match self {
            Self::Small(ids) => ids.add(id),
            Self::Large(ids) => ids.add(id),
        }
    }
}

impl Scope {
    fn is_empty(&self) -> bool {
        self.activity.is_none() && self.book.is_none() && self.bars.is_empty()
    }

    fn complete(
        &mut self,
        feed: Feed,
        result: Result<Batch, Error>,
        instrument: &BinaryOption,
        sender: &EventSender<DataEvent>,
        interval: Duration,
        limit: u32,
    ) -> anyhow::Result<()> {
        let schedule = match feed {
            Feed::Activity => {
                let Some(activity) = &mut self.activity else {
                    return Ok(());
                };

                &mut activity.schedule
            }
            Feed::Book => {
                let Some(book) = &mut self.book else {
                    return Ok(());
                };

                &mut book.schedule
            }
            Feed::Bars(bar_type) => {
                let Some(bars) = self.bars.get_mut(&bar_type) else {
                    return Ok(());
                };

                &mut bars.schedule
            }
        };

        schedule.due = time::Instant::now() + interval;

        let batch = match result {
            Ok(batch) => batch,
            Err(e) => {
                schedule.suspended = permanent(&e);

                log::error!("FinFeed {feed:?} poll failed for {}: {e}", instrument.id);
                return Ok(());
            }
        };

        match batch {
            Batch::Activity(batch) => {
                let Some(activity) = &mut self.activity else {
                    return Ok(());
                };

                let quotes = batch.quotes.as_ref().map_or(0, Vec::len);
                let trades = batch.trades.as_ref().map_or(0, Vec::len);
                let quotes_saturated = activity.quote_ids.is_some() && quotes == limit as usize;
                let trades_saturated = activity.trade_ids.is_some() && trades == limit as usize;
                log::debug!(
                    "FinFeed activity received for {}, quotes={quotes}, trades={trades}, per-list limit={limit}, quotes_saturated={quotes_saturated}, trades_saturated={trades_saturated}",
                    instrument.id
                );

                publish(activity, instrument, batch, sender)
            }
            Batch::Book(source) => {
                let Some(book) = &mut self.book else {
                    return Ok(());
                };

                book.publish(&source, instrument, sender)
            }
            Batch::Bars(bar_type, period, cutoff, records) => {
                let Some(bars) = self.bars.get_mut(&bar_type) else {
                    return Ok(());
                };

                let limit = limit + 1;
                log::debug!(
                    "FinFeed OHLCV received for {bar_type}, records={}, limit={limit}, saturated={}",
                    records.len(),
                    records.len() == limit as usize
                );

                if let Err(e) = bars.publish(records, cutoff, bar_type, &period, instrument, sender)
                {
                    if sender.is_closed() {
                        return Err(e);
                    }

                    log::warn!("Invalid FinFeed OHLCV poll omitted for {bar_type}: {e}");
                }

                Ok(())
            }
        }
    }
}

async fn request(
    state: &Mutex<Publication>,
    http: &FinFeedHttpClient,
    changed: &tokio::sync::Notify,
    cancellation: &CancellationToken,
    scope: (InstrumentId, Feed, u64),
    path: (&str, &str),
    query: &ActivityQuery,
) -> Option<Result<Batch, Error>> {
    let (id, feed, revision) = scope;
    let (exchange, market) = path;

    let future = async {
        {
            let state = state.lock();
            if !state.connected
                || !state
                    .polling
                    .schedule(id, feed)
                    .is_some_and(|schedule| schedule.revision == revision)
            {
                return Err(Error::Response("poll subscription changed"));
            }
        }

        match feed {
            Feed::Activity => http
                .get_latest(exchange, market, query)
                .await
                .map(Batch::Activity),
            Feed::Book => http.get_book(exchange, market).await.map(Batch::Book),
            Feed::Bars(bar_type) => {
                let cached = state
                    .lock()
                    .polling
                    .subscriptions
                    .get(&id)
                    .and_then(|scope| scope.bars.get(&bar_type))
                    .and_then(|bars| bars.period.clone());

                let period = if let Some(period) = cached {
                    period
                } else {
                    let periods = http.get_periods().await?;
                    let length = history::period_length(bar_type)
                        .map_err(|e| Error::Validation(e.to_string()))?;
                    let period = history::period(&periods, length)
                        .map_err(|e| Error::Validation(e.to_string()))?
                        .clone();
                    let mut state = state.lock();
                    if !state.connected {
                        return Err(Error::Response("OHLCV subscription changed"));
                    }

                    let Some(bars) = state
                        .polling
                        .subscriptions
                        .get_mut(&id)
                        .and_then(|scope| scope.bars.get_mut(&bar_type))
                        .filter(|bars| bars.schedule.revision == revision)
                    else {
                        return Err(Error::Response("OHLCV subscription changed"));
                    };

                    bars.period = Some(period.clone());

                    period
                };

                let query = OhlcvQuery {
                    period_id: period
                        .period_id
                        .clone()
                        .ok_or(Error::Response("OHLCV period ID is missing"))?,
                    time_start: None,
                    time_end: None,
                    limit: query.limit + 1,
                };

                let cutoff = get_atomic_clock_realtime().get_time_ns().to_datetime_utc();
                let records = http.get_ohlcv_latest(exchange, market, &query).await?;
                Ok(Batch::Bars(bar_type, period, cutoff, records))
            }
        }
    };

    tokio::pin!(future);

    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return None,
            result = &mut future => return Some(result),
            () = changed.notified() => {
                let state = state.lock();
                if !state.connected || !state.polling.schedule(id, feed).is_some_and(|schedule| schedule.revision == revision) {
                    return None;
                }
            }
        }
    }
}

async fn wait(deadline: Option<time::Instant>) {
    if let Some(deadline) = deadline {
        time::sleep_until(deadline).await;
    } else {
        std::future::pending::<()>().await;
    }
}

fn publish(
    scope: &mut ActivityScope,
    instrument: &BinaryOption,
    mut batch: ActivityBatch,
    sender: &EventSender<DataEvent>,
) -> anyhow::Result<()> {
    if let Some(ids) = &mut scope.quote_ids {
        let records = batch.quotes.get_or_insert_with(Vec::new);
        records.sort_by_key(|quote| quote.recv_time);
        for record in records {
            match parse::quote(record, instrument) {
                Ok(tick) if !ids.contains(&tick) => {
                    send(sender, DataEvent::Data(Data::Quote(tick)))?;
                    ids.add(tick);
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("Invalid FinFeed poll quote omitted: {e}");
                }
            }
        }
    }

    if let Some(ids) = &mut scope.trade_ids {
        let records = batch.trades.get_or_insert_with(Vec::new);
        records.sort_by_key(|trade| trade.timestamp);
        for record in records {
            match parse::trade(record, instrument) {
                Ok(tick) if !ids.contains(&tick.trade_id) => {
                    send(sender, DataEvent::Data(Data::Trade(tick)))?;
                    ids.add(tick.trade_id);
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("Invalid FinFeed poll trade omitted: {e}");
                }
            }
        }
    }

    Ok(())
}

fn permanent(e: &Error) -> bool {
    matches!(
        e,
        Error::AttemptBudget | Error::Validation(_) | Error::Configuration
    ) || matches!(e, Error::Http { status: status @ 400..=499, .. } if !matches!(status, 404 | 408 | 429))
}

#[derive(Debug)]
struct BarScope {
    schedule: Schedule,
    period: Option<OhlcvPeriod>,
    watermark: Timestamp,
}

impl BarScope {
    fn publish(
        &mut self,
        mut records: Vec<Ohlcv>,
        cutoff: Timestamp,
        bar_type: BarType,
        period: &OhlcvPeriod,
        instrument: &BinaryOption,
        sender: &EventSender<DataEvent>,
    ) -> anyhow::Result<()> {
        records.sort_by_key(|row| (row.time_period_end, row.time_period_start));
        let mut bars = Vec::new();
        let mut previous: Option<&Ohlcv> = None;

        for record in &records {
            if record.time_period_end <= self.watermark || record.time_period_end > cutoff {
                continue;
            }

            if let Some(prior) = previous
                && prior.time_period_end == record.time_period_end
            {
                anyhow::ensure!(
                    prior == record,
                    "conflicting FinFeed OHLCV period duplicates"
                );
                continue;
            }

            if let Some(bar) = parse::bar(record, bar_type, instrument, period)? {
                bars.push(bar);
            }

            previous = Some(record);
        }

        for bar in bars {
            send(sender, DataEvent::Data(Data::Bar(bar)))?;
            self.watermark = bar.ts_event.to_datetime_utc();
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
struct BookScope {
    schedule: Schedule,
    published_revision: u64,
    deltas: bool,
    deltas_depth: Option<usize>,
    depth: Option<usize>,
    previous: Option<Snapshot>,
}

impl BookScope {
    fn publish(
        &mut self,
        source: &Book,
        instrument: &BinaryOption,
        sender: &EventSender<DataEvent>,
    ) -> anyhow::Result<()> {
        let snapshot = match Snapshot::parse(source, instrument) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                log::warn!("Invalid FinFeed book snapshot omitted: {e}");
                return Ok(());
            }
        };

        if let Some(previous) = &self.previous {
            if (snapshot.ts_init, snapshot.ts_event) < (previous.ts_init, previous.ts_event) {
                log::warn!(
                    "Stale FinFeed book snapshot omitted for {}, received=({}, {}), retained=({}, {})",
                    instrument.id,
                    snapshot.ts_init,
                    snapshot.ts_event,
                    previous.ts_init,
                    previous.ts_event
                );
                return Ok(());
            }

            if snapshot == *previous && self.published_revision == self.schedule.revision {
                return Ok(());
            }
        }

        let deltas = self
            .deltas
            .then(|| snapshot.deltas(instrument.id, self.deltas_depth))
            .transpose()?;
        let depth = self
            .depth
            .map(|limit| snapshot.depth(instrument.id, limit))
            .transpose()?;

        for data in deltas
            .map(Data::from)
            .into_iter()
            .chain(depth.map(Data::from))
        {
            send(sender, DataEvent::Data(data))?;
        }

        self.previous = Some(snapshot);
        self.published_revision = self.schedule.revision;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    bids: Vec<(Price, Quantity)>,
    asks: Vec<(Price, Quantity)>,
}

impl Snapshot {
    fn parse(source: &Book, instrument: &BinaryOption) -> anyhow::Result<Self> {
        let book = parse::book(source, instrument, None)?;

        Ok(Self {
            ts_event: parse::timestamp(source.time_exchange)?,
            ts_init: parse::timestamp(source.time_coinapi)?,
            bids: book
                .bids(None)
                .flat_map(|level| level.iter().map(|order| (order.price, order.size)))
                .collect(),
            asks: book
                .asks(None)
                .flat_map(|level| level.iter().map(|order| (order.price, order.size)))
                .collect(),
        })
    }

    fn deltas(&self, id: InstrumentId, depth: Option<usize>) -> anyhow::Result<OrderBookDeltas> {
        let mut deltas = vec![OrderBookDelta::clear(id, 0, self.ts_event, self.ts_init)];

        for (side, levels) in [(OrderSide::Buy, &self.bids), (OrderSide::Sell, &self.asks)] {
            for (price, size) in levels.iter().take(depth.unwrap_or(usize::MAX)) {
                let order = BookOrder::new(side, *price, *size, 0);
                deltas.push(OrderBookDelta::new_checked(
                    id,
                    BookAction::Add,
                    order,
                    RecordFlag::F_SNAPSHOT as u8,
                    0,
                    self.ts_event,
                    self.ts_init,
                )?);
            }
        }

        deltas.last_mut().expect("snapshot contains clear").flags |= RecordFlag::F_LAST as u8;
        OrderBookDeltas::new_checked(id, deltas)
    }

    fn depth(&self, id: InstrumentId, depth: usize) -> anyhow::Result<OrderBookDepth> {
        let bids: Vec<_> = self
            .bids
            .iter()
            .take(depth)
            .map(|(price, size)| BookOrder::new(OrderSide::Buy, *price, *size, 0))
            .collect();
        let asks: Vec<_> = self
            .asks
            .iter()
            .take(depth)
            .map(|(price, size)| BookOrder::new(OrderSide::Sell, *price, *size, 0))
            .collect();
        let bid_counts = vec![0; bids.len()];
        let ask_counts = vec![0; asks.len()];

        OrderBookDepth::new_checked(
            id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
            0,
            self.ts_event,
            self.ts_init,
        )
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UnixNanos, string::secret::SecretString};
    use nautilus_model::{
        data::{Bar, BarSpecification, TradeTick},
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::http::{client::FinFeedHttpConfig, models::Market, parse::parse_instrument};

    #[rstest]
    #[case("disconnected")]
    #[case("removed")]
    #[case("revision")]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    async fn invalidated_poll_cannot_start_http_work(#[case] reason: &str) {
        let id = instrument().id;

        let mut publication = Publication {
            connected: true,
            ..Default::default()
        };

        publication.polling.subscribe(id, true, 100).unwrap();
        let revision = publication
            .polling
            .schedule(id, Feed::Activity)
            .unwrap()
            .revision;

        match reason {
            "disconnected" => publication.connected = false,
            "removed" => publication.polling.deactivate(id),
            "revision" => publication.polling.refresh(id),
            _ => unreachable!(),
        }

        let state = Mutex::new(publication);

        let config = FinFeedHttpConfig {
            base_url: "http://127.0.0.1:1/v1/".into(),
            ..Default::default()
        };

        let http =
            FinFeedHttpClient::new(Some(SecretString::from("test-api-key")), config).unwrap();
        let remaining = http.attempts_remaining();
        let changed = tokio::sync::Notify::new();
        let cancellation = CancellationToken::new();
        let result = Box::pin(time::timeout(
            Duration::from_millis(100),
            request(
                &state,
                &http,
                &changed,
                &cancellation,
                (id, Feed::Activity, revision),
                ("POLYMARKET", "contract_NO"),
                &ActivityQuery { limit: 1 },
            ),
        ))
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            result,
            Err(Error::Response("poll subscription changed"))
        ));
        assert_eq!(http.attempts_remaining(), remaining);
    }

    #[rstest]
    fn subscription_bound_and_revisions_preserve_existing_scopes() {
        let mut polling = Polling::default();

        for index in 0..SUBSCRIPTIONS_MAX {
            let id = InstrumentId::from(format!("contract{index}_NO.FINFEED_POLYMARKET").as_str());
            assert!(polling.subscribe(id, true, 100).unwrap());
        }

        let existing = InstrumentId::from("contract0_NO.FINFEED_POLYMARKET");
        let next = InstrumentId::from("contract128_NO.FINFEED_POLYMARKET");
        let revision = polling.subscriptions[&existing]
            .activity
            .as_ref()
            .unwrap()
            .schedule
            .revision;
        assert!(!polling.subscribe(existing, true, 100).unwrap());
        assert_eq!(
            polling.subscriptions[&existing]
                .activity
                .as_ref()
                .unwrap()
                .schedule
                .revision,
            revision
        );
        assert!(polling.subscribe(existing, false, 100).unwrap());
        assert!(polling.subscribe(next, true, 100).is_err());
        assert_eq!(polling.subscriptions.len(), SUBSCRIPTIONS_MAX);
        assert!(polling.unsubscribe(existing, true));
        assert!(
            polling.subscriptions[&existing]
                .activity
                .as_ref()
                .unwrap()
                .trade_ids
                .is_some()
        );
        assert!(polling.unsubscribe(existing, false));
        assert!(polling.subscribe(next, true, 100).unwrap());
        assert_eq!(polling.subscriptions.len(), SUBSCRIPTIONS_MAX);
    }

    #[rstest]
    fn book_and_activity_feeds_share_the_instrument_limit() {
        let mut polling = Polling::default();

        for index in 0..SUBSCRIPTIONS_MAX {
            let id = InstrumentId::from(format!("contract{index}_NO.FINFEED_POLYMARKET").as_str());
            assert!(polling.subscribe_book(id, true, None).unwrap());
        }

        let existing = InstrumentId::from("contract0_NO.FINFEED_POLYMARKET");
        let next = InstrumentId::from("contract128_NO.FINFEED_POLYMARKET");
        assert!(polling.subscribe(existing, true, 100).unwrap());
        assert!(polling.subscribe_book(existing, false, Some(1)).unwrap());
        assert_eq!(
            polling
                .subscribe_book(next, true, None)
                .unwrap_err()
                .to_string(),
            "FinFeed subscription limit exceeded"
        );
        assert_eq!(
            polling.subscribe(next, true, 100).unwrap_err().to_string(),
            "FinFeed subscription limit exceeded"
        );
        assert!(polling.unsubscribe_book(existing, true));
        assert!(polling.unsubscribe_book(existing, false));
        assert_eq!(
            polling
                .subscribe_book(next, false, Some(1))
                .unwrap_err()
                .to_string(),
            "FinFeed subscription limit exceeded"
        );
        assert!(polling.unsubscribe(existing, true));
        assert!(polling.subscribe_book(next, false, Some(1)).unwrap());
        assert_eq!(polling.instrument_ids().len(), SUBSCRIPTIONS_MAX);
    }

    #[rstest]
    fn quote_identity_includes_each_field_and_retention_is_bounded() {
        let instrument = instrument();

        let mut scope = ActivityScope {
            quote_ids: Some(Identities::new(PAGE_LIMIT_MAX)),
            trade_ids: Some(Identities::new(PAGE_LIMIT_MAX)),
            ..Default::default()
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let mut batch: ActivityBatch =
            serde_json::from_str(include_str!("../../test_data/poll-activity.json")).unwrap();
        let original = batch.quotes.as_ref().unwrap()[0].clone();
        batch.quotes = Some(vec![original.clone()]);
        batch.trades = None;
        publish(&mut scope, &instrument, batch.clone(), &sender).unwrap();
        let mut variants = Vec::new();

        for field in 0..6 {
            let mut quote = original.clone();
            match field {
                0 => quote.bid += rust_decimal::Decimal::new(1, 4),
                1 => quote.ask += rust_decimal::Decimal::new(1, 4),
                2 => quote.bid_volume += rust_decimal::Decimal::new(1, 6),
                3 => quote.ask_volume += rust_decimal::Decimal::new(1, 6),
                4 => {
                    quote.entry_time =
                        jiff::Timestamp::from_nanosecond(quote.entry_time.as_nanosecond() + 1)
                            .unwrap();
                }
                5 => {
                    quote.recv_time =
                        jiff::Timestamp::from_nanosecond(quote.recv_time.as_nanosecond() + 1)
                            .unwrap();
                }
                _ => unreachable!(),
            }

            variants.push(quote);
        }

        batch.quotes = Some(variants);
        publish(&mut scope, &instrument, batch.clone(), &sender).unwrap();
        publish(&mut scope, &instrument, batch, &sender).unwrap();
        assert_eq!(receiver.len(), 7);

        while receiver.try_recv().is_ok() {}

        for index in 0..=IDENTITIES_MAX {
            let mut quote = original.clone();
            quote.recv_time = jiff::Timestamp::from_nanosecond(
                quote.recv_time.as_nanosecond() + 10 + index as i128,
            )
            .unwrap();
            publish(
                &mut scope,
                &instrument,
                ActivityBatch {
                    quotes: Some(vec![quote]),
                    trades: None,
                },
                &sender,
            )
            .unwrap();
        }

        assert_eq!(identities_len(&scope.quote_ids), IDENTITIES_MAX);
        assert!(
            !scope
                .quote_ids
                .as_ref()
                .unwrap()
                .contains(&parse::quote(&original, &instrument).unwrap())
        );
        assert_eq!(receiver.len(), IDENTITIES_MAX + 1);
    }

    #[rstest]
    fn activity_identity_retention_covers_a_full_batch_and_fifo_eviction() {
        let instrument = instrument();
        let limit = u64::from(PAGE_LIMIT_MAX);
        let mut polling = Polling::default();
        polling
            .subscribe(instrument.id, true, PAGE_LIMIT_MAX)
            .unwrap();
        polling
            .subscribe(instrument.id, false, PAGE_LIMIT_MAX)
            .unwrap();
        let activity = polling
            .subscriptions
            .get_mut(&instrument.id)
            .unwrap()
            .activity
            .as_mut()
            .unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();

        for range in [0..limit, limit..2 * limit] {
            let (batch, expected) = activity_batch(range);
            publish(activity, &instrument, batch.clone(), &sender).unwrap();
            assert_eq!(drain_activity(&mut receiver), expected);
            publish(activity, &instrument, batch, &sender).unwrap();
            assert!(receiver.try_recv().is_err());
        }

        assert_eq!(identities_len(&activity.quote_ids), IDENTITIES_MAX);
        assert_eq!(identities_len(&activity.trade_ids), IDENTITIES_MAX);
        let (next, expected) = activity_batch(2 * limit..2 * limit + 1);
        publish(activity, &instrument, next, &sender).unwrap();
        assert_eq!(drain_activity(&mut receiver), expected);
        let (oldest, expected) = activity_batch(0..1);
        let oldest_quote = parse::quote(&oldest.quotes.as_ref().unwrap()[0], &instrument).unwrap();
        assert!(!activity.quote_ids.as_ref().unwrap().contains(&oldest_quote));
        assert!(
            !activity
                .trade_ids
                .as_ref()
                .unwrap()
                .contains(&TradeId::from("retention-0"))
        );
        assert!(
            activity
                .trade_ids
                .as_ref()
                .unwrap()
                .contains(&TradeId::from("retention-1"))
        );
        publish(activity, &instrument, oldest, &sender).unwrap();
        assert_eq!(drain_activity(&mut receiver), expected);
        assert_eq!(identities_len(&activity.quote_ids), IDENTITIES_MAX);
        assert_eq!(identities_len(&activity.trade_ids), IDENTITIES_MAX);
        assert!(polling.unsubscribe(instrument.id, true));
        let activity = polling.subscriptions[&instrument.id]
            .activity
            .as_ref()
            .unwrap();
        assert!(activity.quote_ids.is_none());
        assert_eq!(identities_len(&activity.trade_ids), IDENTITIES_MAX);
        assert!(polling.unsubscribe(instrument.id, false));
        assert_eq!(polling.subscriptions.len(), 0);
    }

    #[rstest]
    #[case(1_000, 1)]
    #[case(10_000, 1)]
    #[case(1_024, 1_023)]
    #[case(10_000, 9_999)]
    fn sliding_activity_window_keeps_late_record_identity(
        #[case] limit: u32,
        #[case] advance: u64,
    ) {
        let instrument = instrument();
        let mut polling = Polling::default();
        polling.subscribe(instrument.id, true, limit).unwrap();
        polling.subscribe(instrument.id, false, limit).unwrap();
        let activity = polling
            .subscriptions
            .get_mut(&instrument.id)
            .unwrap()
            .activity
            .as_mut()
            .unwrap();
        let limit = u64::from(limit);
        let (mut batch, expected) = activity_batch(0..limit);
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        publish(activity, &instrument, batch.clone(), &sender).unwrap();
        assert_eq!(drain_activity(&mut receiver), expected);
        batch.quotes.as_mut().unwrap().drain(1..=advance as usize);
        batch.trades.as_mut().unwrap().drain(1..=advance as usize);
        let (next, expected) = activity_batch(limit..limit + advance);
        batch.quotes.as_mut().unwrap().extend(next.quotes.unwrap());
        batch.trades.as_mut().unwrap().extend(next.trades.unwrap());
        publish(activity, &instrument, batch.clone(), &sender).unwrap();
        assert_eq!(drain_activity(&mut receiver), expected);
        publish(activity, &instrument, batch, &sender).unwrap();
        assert!(
            receiver.try_recv().is_err(),
            "unchanged sliding window must not replay its late record"
        );
    }

    #[rstest]
    #[case(100, IDENTITIES_MIN)]
    #[case(1_000, IDENTITIES_MIN)]
    #[case(1_001, IDENTITIES_MIN)]
    #[case(1_024, IDENTITIES_MIN)]
    #[case(1_025, IDENTITIES_MAX)]
    #[case(10_000, IDENTITIES_MAX)]
    fn identity_capacity_tracks_limit_and_only_subscribed_lists_allocate(
        #[case] limit: u32,
        #[case] capacity: usize,
    ) {
        let id = instrument().id;
        let mut polling = Polling::default();
        polling.subscribe(id, true, limit).unwrap();
        let activity = polling.subscriptions[&id].activity.as_ref().unwrap();
        assert_eq!(identities_capacity(&activity.quote_ids), capacity);
        assert!(activity.trade_ids.is_none());
        polling.subscribe(id, false, limit).unwrap();
        let activity = polling.subscriptions[&id].activity.as_ref().unwrap();
        assert_eq!(identities_capacity(&activity.quote_ids), capacity);
        assert_eq!(identities_capacity(&activity.trade_ids), capacity);
        polling.unsubscribe(id, true);
        let activity = polling.subscriptions[&id].activity.as_ref().unwrap();
        assert!(activity.quote_ids.is_none());
        assert_eq!(identities_capacity(&activity.trade_ids), capacity);
    }

    fn identities_capacity<T: Clone + Debug + Eq + Hash>(ids: &Option<Identities<T>>) -> usize {
        match ids {
            Some(Identities::Small(ids)) => ids.capacity(),
            Some(Identities::Large(ids)) => ids.capacity(),
            None => 0,
        }
    }

    fn activity_batch(range: std::ops::Range<u64>) -> (ActivityBatch, Vec<Data>) {
        let instrument = instrument();
        let template: ActivityBatch =
            serde_json::from_str(include_str!("../../test_data/poll-activity.json")).unwrap();

        let mut batch = ActivityBatch {
            quotes: Some(Vec::new()),
            trades: Some(Vec::new()),
        };

        let mut quotes = Vec::new();
        let mut trades = Vec::new();

        for index in range {
            let ts_event = UnixNanos::from(1_791_676_800_000_000_000 + (index + 1) * 1_000_000_000);
            let ts_init = UnixNanos::from(1_791_676_800_000_000_000 + (index + 2) * 1_000_000_000);
            let mut quote = template.quotes.as_ref().unwrap()[0].clone();
            quote.entry_time = book_timestamp(index + 1);
            quote.recv_time = book_timestamp(index + 2);
            batch.quotes.as_mut().unwrap().push(quote);
            quotes.push(Data::Quote(
                QuoteTick::new_checked(
                    instrument.id,
                    Price::from("0.3000"),
                    Price::from("0.4000"),
                    Quantity::from("3.000003"),
                    Quantity::from("4.000004"),
                    ts_event,
                    ts_init,
                )
                .unwrap(),
            ));
            let mut trade = template.trades.as_ref().unwrap()[0].clone();
            trade.id = Some(format!("retention-{index}"));
            trade.timestamp = book_timestamp(index + 2);
            batch.trades.as_mut().unwrap().push(trade);
            trades.push(Data::Trade(
                TradeTick::new_checked(
                    instrument.id,
                    Price::from("0.3100"),
                    Quantity::from("2.000002"),
                    AggressorSide::NoAggressor,
                    TradeId::from(format!("retention-{index}")),
                    ts_init,
                    ts_init,
                )
                .unwrap(),
            ));
        }

        (batch, quotes.into_iter().chain(trades).collect())
    }

    fn drain_activity(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Vec<Data> {
        std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|event| match event {
                DataEvent::Data(data) => data,
                _ => panic!("expected activity"),
            })
            .collect()
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn closed_receiver_does_not_consume_dedup_identity(#[case] quotes: bool) {
        let instrument = instrument();

        let mut scope = ActivityScope {
            quote_ids: quotes.then(|| Identities::new(100)),
            trade_ids: (!quotes).then(|| Identities::new(100)),
            ..Default::default()
        };

        let batch: ActivityBatch =
            serde_json::from_str(include_str!("../../test_data/poll-activity.json")).unwrap();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        drop(receiver);
        assert_eq!(
            publish(&mut scope, &instrument, batch, &sender.into())
                .unwrap_err()
                .to_string(),
            "FinFeed data receiver is closed"
        );
        assert_eq!(identities_len(&scope.quote_ids), 0);
        assert_eq!(identities_len(&scope.trade_ids), 0);
    }

    fn identities_len<T: Clone + Debug + Eq + Hash>(ids: &Option<Identities<T>>) -> usize {
        match ids {
            Some(Identities::Small(ids)) => ids.len(),
            Some(Identities::Large(ids)) => ids.len(),
            None => 0,
        }
    }

    #[rstest]
    fn book_identity_preserves_same_time_size_changes_and_revision_bootstrap() {
        let instrument = instrument();

        let mut scope = BookScope {
            schedule: Schedule {
                revision: 1,
                ..Default::default()
            },
            deltas: true,
            depth: Some(1),
            ..Default::default()
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let mut source: Book =
            serde_json::from_str(include_str!("../../test_data/poll-book.json")).unwrap();
        scope.publish(&source, &instrument, &sender).unwrap();

        let DataEvent::Data(Data::BookDeltas(deltas)) = receiver.try_recv().unwrap() else {
            panic!("expected complete book snapshot")
        };

        assert_eq!(deltas.instrument_id, instrument.id);
        assert_eq!(deltas.sequence, 0);
        assert_eq!(deltas.ts_event, UnixNanos::from(1_791_676_801_000_000_000));
        assert_eq!(deltas.ts_init, UnixNanos::from(1_791_676_802_000_000_000));
        assert_eq!(deltas.deltas.len(), 6);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].order.price.to_string(), "0.3000");
        assert_eq!(deltas.deltas[1].order.size.to_string(), "1.000001");
        assert_eq!(deltas.deltas[1].order.order_id, 0);
        assert_eq!(
            deltas.deltas[5].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );

        let DataEvent::Data(Data::BookDepth(depth)) = receiver.try_recv().unwrap() else {
            panic!("expected limited book snapshot")
        };

        assert_eq!(depth.bids.len(), 1);
        assert_eq!(depth.asks.len(), 1);
        assert_eq!(depth.bid_counts.as_slice(), &[0]);
        assert_eq!(depth.ask_counts.as_slice(), &[0]);
        assert_eq!(depth.bids[0].price.to_string(), "0.3000");
        assert_eq!(depth.asks[0].price.to_string(), "0.4000");

        scope.publish(&source, &instrument, &sender).unwrap();
        assert!(receiver.try_recv().is_err());
        source.bids.as_mut().unwrap()[1].size += rust_decimal::Decimal::new(1, 6);
        scope.publish(&source, &instrument, &sender).unwrap();

        let DataEvent::Data(Data::BookDeltas(changed)) = receiver.try_recv().unwrap() else {
            panic!("expected changed book size")
        };

        assert_eq!(changed.deltas[1].order.size.to_string(), "1.000002");

        let DataEvent::Data(Data::BookDepth(changed)) = receiver.try_recv().unwrap() else {
            panic!("expected changed depth size")
        };

        assert_eq!(changed.bids[0].size.to_string(), "1.000002");

        scope.schedule.revision = 2;
        scope.publish(&source, &instrument, &sender).unwrap();
        assert_eq!(receiver.len(), 2);
        assert_eq!(scope.published_revision, 2);
        assert_eq!(
            scope.previous.as_ref().unwrap().bids[0].1.to_string(),
            "1.000002"
        );
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn book_commands_preserve_revision_and_partial_unsubscribe_preserves_suspension(
        #[case] deltas: bool,
    ) {
        let id = instrument().id;
        let mut polling = Polling::default();
        assert!(polling.subscribe_book(id, deltas, Some(1)).unwrap());
        let initial = polling.subscriptions[&id]
            .book
            .as_ref()
            .unwrap()
            .schedule
            .revision;
        let due = polling.subscriptions[&id]
            .book
            .as_ref()
            .unwrap()
            .schedule
            .due;
        assert!(!polling.subscribe_book(id, deltas, Some(1)).unwrap());
        assert_eq!(
            polling.subscriptions[&id]
                .book
                .as_ref()
                .unwrap()
                .schedule
                .revision,
            initial
        );
        assert_eq!(
            polling.subscriptions[&id]
                .book
                .as_ref()
                .unwrap()
                .schedule
                .due,
            due
        );
        assert!(polling.subscribe_book(id, !deltas, Some(1)).unwrap());
        polling
            .subscriptions
            .get_mut(&id)
            .unwrap()
            .book
            .as_mut()
            .unwrap()
            .schedule
            .suspended = true;
        assert!(polling.unsubscribe_book(id, deltas));
        let book = polling.subscriptions[&id].book.as_ref().unwrap();
        assert!(book.schedule.suspended);
        assert_eq!(book.deltas, !deltas);
        assert_eq!(book.depth, if deltas { Some(1) } else { None });
        assert_eq!(polling.next(), None);
        assert!(polling.subscribe_book(id, !deltas, Some(1)).unwrap());
        assert!(
            !polling.subscriptions[&id]
                .book
                .as_ref()
                .unwrap()
                .schedule
                .suspended
        );
        assert!(polling.next().is_some());
    }

    #[rstest]
    #[case("quotes")]
    #[case("trades")]
    #[case("activity")]
    #[case("deltas")]
    #[case("depth")]
    #[case("book")]
    fn unsubscribing_an_inactive_feed_keeps_the_active_schedule(#[case] inactive: &str) {
        let id = instrument().id;
        let mut polling = Polling::default();
        let (subscribed, feed) = match inactive {
            "quotes" => (polling.subscribe(id, false, 100), Feed::Activity),
            "trades" | "book" => (polling.subscribe(id, true, 100), Feed::Activity),
            "deltas" => (polling.subscribe_book(id, false, Some(5)), Feed::Book),
            "depth" | "activity" => (polling.subscribe_book(id, true, None), Feed::Book),
            _ => unreachable!(),
        };
        assert!(subscribed.unwrap());
        let revision = polling.revision;
        let next = polling.next();
        assert_eq!(
            next.map(|(next_id, next_feed, _)| (next_id, next_feed)),
            Some((id, feed))
        );

        let changed = match inactive {
            "quotes" => polling.unsubscribe(id, true),
            "trades" | "activity" => polling.unsubscribe(id, false),
            "deltas" | "book" => polling.unsubscribe_book(id, true),
            "depth" => polling.unsubscribe_book(id, false),
            _ => unreachable!(),
        };

        assert!(!changed);
        assert_eq!(polling.revision, revision);
        assert_eq!(polling.schedule(id, feed).unwrap().revision, revision);
        assert_eq!(polling.next(), next);
    }

    #[rstest]
    #[case(0, 2, false)]
    #[case(0, 3, true)]
    #[case(3, 1, false)]
    #[case(2, 2, true)]
    #[case(1, 2, false)]
    #[case(2, 3, true)]
    fn book_watermark_and_identity_use_provider_receive_then_exchange_time(
        #[case] exchange: u64,
        #[case] receive: u64,
        #[case] accepted: bool,
    ) {
        let instrument = instrument();

        let mut scope = BookScope {
            deltas: true,
            ..Default::default()
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let mut source: Book =
            serde_json::from_str(include_str!("../../test_data/poll-book.json")).unwrap();
        scope.publish(&source, &instrument, &sender).unwrap();
        receiver.try_recv().unwrap();
        source.time_exchange = book_timestamp(exchange);
        source.time_coinapi = book_timestamp(receive);
        scope.publish(&source, &instrument, &sender).unwrap();
        assert_eq!(receiver.len(), usize::from(accepted));
        let previous = scope.previous.as_ref().unwrap();
        assert_eq!(
            previous.ts_event,
            parse::timestamp(book_timestamp(if accepted { exchange } else { 1 })).unwrap()
        );
        assert_eq!(
            previous.ts_init,
            parse::timestamp(book_timestamp(if accepted { receive } else { 2 })).unwrap()
        );

        if accepted {
            let DataEvent::Data(Data::BookDeltas(deltas)) = receiver.try_recv().unwrap() else {
                panic!("expected fresh snapshot")
            };

            assert_eq!(
                deltas.ts_event,
                parse::timestamp(source.time_exchange).unwrap()
            );
            assert_eq!(
                deltas.ts_init,
                parse::timestamp(source.time_coinapi).unwrap()
            );
            assert_eq!(deltas.deltas[1].order.price.to_string(), "0.3000");
            assert_eq!(deltas.deltas[1].order.size.to_string(), "1.000001");
        }
    }

    #[rstest]
    #[case("price")]
    #[case("size")]
    #[case("missing_side")]
    #[case("duplicate_price")]
    #[case("crossed")]
    #[case("identity")]
    #[case("timestamp")]
    fn invalid_book_is_atomic_even_outside_every_requested_depth(#[case] invalid: &str) {
        let instrument = instrument();

        let mut scope = BookScope {
            deltas: true,
            deltas_depth: Some(1),
            depth: Some(1),
            ..Default::default()
        };

        let original: Book =
            serde_json::from_str(include_str!("../../test_data/poll-book.json")).unwrap();
        let mut source = original.clone();

        match invalid {
            "price" => {
                source.bids.as_mut().unwrap()[2].price = rust_decimal::Decimal::new(10001, 5);
            }
            "size" => source.bids.as_mut().unwrap()[2].size = rust_decimal::Decimal::NEGATIVE_ONE,
            "missing_side" => source.asks = None,
            "duplicate_price" => {
                source.bids.as_mut().unwrap()[2].price = rust_decimal::Decimal::new(2, 1);
            }
            "crossed" => source.bids.as_mut().unwrap()[1].price = rust_decimal::Decimal::new(6, 1),
            "identity" => source.market_id = Some("other_NO".into()),
            "timestamp" => source.time_coinapi = jiff::Timestamp::from_nanosecond(-1).unwrap(),
            _ => unreachable!(),
        }

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        scope.publish(&source, &instrument, &sender).unwrap();
        assert!(receiver.try_recv().is_err());
        assert!(scope.previous.is_none());
        scope.publish(&original, &instrument, &sender).unwrap();
        assert_eq!(receiver.len(), 2);
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn closed_book_receiver_does_not_consume_identity(#[case] deltas: bool) {
        let instrument = instrument();

        let mut scope = BookScope {
            deltas,
            depth: (!deltas).then_some(1),
            ..Default::default()
        };

        let source: Book =
            serde_json::from_str(include_str!("../../test_data/poll-book.json")).unwrap();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        drop(receiver);
        let result = scope.publish(&source, &instrument, &sender.into());
        assert_eq!(
            result.unwrap_err().to_string(),
            "FinFeed data receiver is closed"
        );
        assert!(scope.previous.is_none());
        assert_eq!(scope.published_revision, 0);
    }

    #[rstest]
    fn closed_bars_use_cutoff_boundaries_and_exact_shared_conversion() {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let period = minute_period();
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let start = records[1].time_period_start;
        let boundary = records[1].time_period_end;

        let mut scope = BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: start,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let before = Timestamp::from_nanosecond(boundary.as_nanosecond() - 1).unwrap();
        scope
            .publish(
                records.clone(),
                before,
                bar_type,
                &period,
                &instrument,
                &sender,
            )
            .unwrap();
        assert_eq!(scope.watermark, start);
        assert!(receiver.try_recv().is_err());

        let mut final_records = records.clone();
        final_records[1].price_close = Some(Decimal::new(35, 2));
        final_records[1].volume_traded = Decimal::from_str_exact("12.345678000000001").unwrap();
        scope
            .publish(
                final_records.clone(),
                boundary,
                bar_type,
                &period,
                &instrument,
                &sender,
            )
            .unwrap();

        let DataEvent::Data(Data::Bar(first)) = receiver.try_recv().unwrap() else {
            panic!("expected bar")
        };

        let expected = Bar::new_checked(
            bar_type,
            Price::from("0.2000"),
            Price::from("0.4000"),
            Price::from("0.1000"),
            Price::from("0.3500"),
            Quantity::from("12.345678"),
            parse::timestamp(boundary).unwrap(),
            parse::timestamp(boundary).unwrap(),
        )
        .unwrap();
        assert_eq!(first, expected);
        assert_eq!(
            Some(first),
            parse::bar(&final_records[1], bar_type, &instrument, &period).unwrap()
        );
        assert_eq!(scope.watermark, boundary);
        assert!(receiver.try_recv().is_err());

        let end = records[2].time_period_end;
        let mut repeated = final_records.clone();
        repeated.push(final_records[1].clone());
        scope
            .publish(repeated, end, bar_type, &period, &instrument, &sender)
            .unwrap();

        for row in [&records[0], &records[2]] {
            let expected = parse::bar(row, bar_type, &instrument, &period)
                .unwrap()
                .unwrap();

            let DataEvent::Data(Data::Bar(actual)) = receiver.try_recv().unwrap() else {
                panic!("expected bar")
            };

            assert_eq!(actual, expected);
        }

        scope
            .publish(records, end, bar_type, &period, &instrument, &sender)
            .unwrap();
        assert_eq!(scope.watermark, end);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    #[case("duration")]
    #[case("open_missing")]
    #[case("high")]
    #[case("low")]
    #[case("price_precision")]
    #[case("volume_precision")]
    #[case("volume_negative")]
    #[case("volume_overflow")]
    #[case("trades_negative")]
    #[case("time_open")]
    #[case("times_reversed")]
    #[case("duplicate_conflict")]
    #[case("duplicate_volume_noise")]
    fn invalid_bar_batches_are_atomic_and_do_not_consume_boundary(#[case] invalid: &str) {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let period = minute_period();
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let start = records[1].time_period_start;
        let end = records[2].time_period_end;
        let mut bad = records.clone();
        let row = &mut bad[1];

        match invalid {
            "duration" => {
                row.time_period_start =
                    Timestamp::from_nanosecond(start.as_nanosecond() + 1).unwrap();
            }
            "open_missing" => row.price_open = None,
            "high" => row.price_high = Some(Decimal::new(2, 1)),
            "low" => row.price_low = Some(Decimal::new(4, 1)),
            "price_precision" => row.price_open = Some(Decimal::new(20001, 5)),
            "volume_precision" => row.volume_traded = Decimal::new(123456789, 7),
            "volume_negative" => row.volume_traded = Decimal::NEGATIVE_ONE,
            "volume_overflow" => row.volume_traded = Decimal::MAX,
            "trades_negative" => row.trades_count = -1,
            "time_open" => row.time_open = Some(end),
            "times_reversed" => {
                row.time_open = row
                    .time_close
                    .map(|t| Timestamp::from_nanosecond(t.as_nanosecond() + 1).unwrap());
            }
            "duplicate_volume_noise" => {
                let mut duplicate = row.clone();
                duplicate.volume_traded = Decimal::from_str_exact("12.345678000000001").unwrap();
                bad.push(duplicate);
            }
            "duplicate_conflict" => {
                let mut duplicate = row.clone();
                duplicate.price_close = Some(Decimal::new(35, 2));
                bad.push(duplicate);
            }
            _ => unreachable!(),
        }

        let mut scope = BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: start,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        assert!(
            scope
                .publish(bad, end, bar_type, &period, &instrument, &sender)
                .is_err()
        );
        assert_eq!(scope.watermark, start);
        assert!(receiver.try_recv().is_err());
        scope
            .publish(records, end, bar_type, &period, &instrument, &sender)
            .unwrap();
        assert_eq!(scope.watermark, end);
        assert_eq!(receiver.len(), 3);
    }

    #[rstest]
    fn invalid_bar_poll_is_omitted_until_the_receiver_closes() {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let start = records[1].time_period_start;
        let end = records[2].time_period_end;
        let mut conflicting = records[1].clone();
        conflicting.price_close = Some(Decimal::new(35, 2));
        let mut invalid = records;
        invalid.push(conflicting);

        let mut polling = Polling::default();
        polling.subscribe_bars(bar_type, start).unwrap();
        let scope = polling.subscriptions.get_mut(&instrument.id).unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let batch = || Ok(Batch::Bars(bar_type, minute_period(), end, invalid.clone()));

        scope
            .complete(
                Feed::Bars(bar_type),
                batch(),
                &instrument,
                &sender,
                Duration::from_millis(100),
                100,
            )
            .unwrap();
        assert_eq!(scope.bars[&bar_type].watermark, start);
        assert!(!scope.bars[&bar_type].schedule.suspended);
        assert!(receiver.try_recv().is_err());

        drop(receiver);
        let error = scope
            .complete(
                Feed::Bars(bar_type),
                batch(),
                &instrument,
                &sender,
                Duration::from_millis(100),
                100,
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "conflicting FinFeed OHLCV period duplicates"
        );
        assert_eq!(scope.bars[&bar_type].watermark, start);
    }

    #[rstest]
    fn identical_period_duplicates_publish_once() {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let period = minute_period();
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let start = records[1].time_period_start;
        let end = records[2].time_period_end;
        let mut repeated = records.clone();
        repeated.push(records[0].clone());

        let mut scope = BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: start,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        scope
            .publish(
                repeated,
                end,
                bar_type,
                &period,
                &instrument,
                &sender.into(),
            )
            .unwrap();

        let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|event| {
                let DataEvent::Data(Data::Bar(bar)) = event else {
                    panic!("expected bar")
                };

                bar
            })
            .collect();

        let expected = [1, 0, 2].map(|index| {
            parse::bar(&records[index], bar_type, &instrument, &period)
                .unwrap()
                .unwrap()
        });

        assert_eq!(events, expected);
        assert_eq!(scope.watermark, end);
    }

    #[rstest]
    fn missing_periods_are_not_fabricated_and_late_periods_stay_behind_watermark() {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let period = minute_period();
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let start = records[1].time_period_start;
        let end = records[2].time_period_end;

        let mut scope = BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: start,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        scope
            .publish(
                vec![records[2].clone(), records[1].clone()],
                end,
                bar_type,
                &period,
                &instrument,
                &sender,
            )
            .unwrap();

        let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|event| {
                let DataEvent::Data(Data::Bar(bar)) = event else {
                    panic!("expected bar")
                };

                bar
            })
            .collect();

        let expected = [1, 2].map(|index| {
            parse::bar(&records[index], bar_type, &instrument, &period)
                .unwrap()
                .unwrap()
        });

        assert_eq!(events, expected);
        scope
            .publish(
                vec![records[0].clone()],
                end,
                bar_type,
                &period,
                &instrument,
                &sender,
            )
            .unwrap();
        assert_eq!(scope.watermark, end);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn closed_bar_receiver_leaves_watermark_unchanged() {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let period = minute_period();
        let records: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let start = records[1].time_period_start;
        let end = records[2].time_period_end;

        let mut scope = BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: start,
        };

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        drop(receiver);
        assert_eq!(
            scope
                .publish(records, end, bar_type, &period, &instrument, &sender.into())
                .unwrap_err()
                .to_string(),
            "FinFeed data receiver is closed"
        );
        assert_eq!(scope.watermark, start);
    }

    #[rstest]
    fn bar_scopes_retain_watermarks_on_restore_and_share_bounds_with_other_feeds() {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let start = Timestamp::from_second(100).unwrap();
        let mut polling = Polling::default();
        assert!(polling.subscribe_bars(bar_type, start).unwrap());
        assert!(
            !polling
                .subscribe_bars(bar_type, Timestamp::from_second(200).unwrap())
                .unwrap()
        );
        assert!(polling.subscribe(instrument.id, true, 100).unwrap());
        assert!(polling.subscribe_book(instrument.id, true, None).unwrap());
        polling
            .subscriptions
            .get_mut(&instrument.id)
            .unwrap()
            .bars
            .get_mut(&bar_type)
            .unwrap()
            .period = Some(minute_period());
        polling.restore();
        let scope = &polling.subscriptions[&instrument.id].bars[&bar_type];
        assert_eq!(scope.watermark, start);
        assert!(scope.period.is_none());
        assert!(!scope.schedule.suspended);
        assert!(polling.unsubscribe(instrument.id, true));
        assert!(polling.unsubscribe_book(instrument.id, true));
        assert_eq!(polling.instrument_ids(), vec![instrument.id]);
        assert!(polling.unsubscribe_bars(bar_type));
        assert!(!polling.unsubscribe_bars(bar_type));
        assert_eq!(polling.instrument_ids(), Vec::new());
        let restart = Timestamp::from_second(300).unwrap();
        assert!(polling.subscribe_bars(bar_type, restart).unwrap());
        assert_eq!(
            polling.subscriptions[&instrument.id].bars[&bar_type].watermark,
            restart
        );

        for index in 1..SUBSCRIPTIONS_MAX {
            let spec = BarSpecification::new(index, BarAggregation::Day, PriceType::Last);
            let other = BarType::new(instrument.id, spec, AggregationSource::External);
            polling.subscribe_bars(other, start).unwrap();
        }

        let spec =
            BarSpecification::new(SUBSCRIPTIONS_MAX + 1, BarAggregation::Day, PriceType::Last);
        let other = BarType::new(instrument.id, spec, AggregationSource::External);
        assert_eq!(
            polling
                .subscribe_bars(other, start)
                .unwrap_err()
                .to_string(),
            "FinFeed bar subscription limit exceeded"
        );
    }

    #[rstest]
    #[case("2020-01-01T00:00:00Z", "2020-02-01T00:00:00Z")]
    #[case("2020-02-01T00:00:00Z", "2020-03-01T00:00:00Z")]
    fn monthly_bars_use_calendar_duration_and_preserve_zero_provider_volume(
        #[case] start: &str,
        #[case] end: &str,
    ) {
        let instrument = instrument();
        let bar_type = BarType::from(format!("{}-1-MONTH-LAST-EXTERNAL", instrument.id).as_str());
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../../test_data/periods.json")).unwrap();
        let period = history::period(&periods, (0, 1)).unwrap();
        let mut rows: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let row = &mut rows[0];
        row.time_period_start = start.parse().unwrap();
        row.time_period_end = end.parse().unwrap();
        row.time_open = None;
        row.time_close = None;
        row.trades_count = 0;
        row.volume_traded = Decimal::ZERO;

        let mut scope = BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: row.time_period_start,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        scope
            .publish(
                vec![row.clone()],
                row.time_period_end,
                bar_type,
                period,
                &instrument,
                &sender.into(),
            )
            .unwrap();

        let DataEvent::Data(Data::Bar(bar)) = receiver.try_recv().unwrap() else {
            panic!("expected bar")
        };

        let expected = Bar::new_checked(
            bar_type,
            Price::from("0.2000"),
            Price::from("0.4000"),
            Price::from("0.1000"),
            Price::from("0.3000"),
            Quantity::from("0.000000"),
            parse::timestamp(row.time_period_end).unwrap(),
            parse::timestamp(row.time_period_end).unwrap(),
        )
        .unwrap();
        assert_eq!(bar, expected);
        assert_eq!(scope.watermark, row.time_period_end);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn empty_periods_do_not_publish_or_consume_the_watermark() {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let period = minute_period();
        let rows: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
        let original = rows[1].clone();
        let mut empty = original.clone();
        empty.price_open = None;
        empty.price_high = None;
        empty.price_low = None;
        empty.price_close = None;
        empty.volume_traded = Decimal::ZERO;
        empty.trades_count = 0;

        let mut scope = BarScope {
            schedule: Schedule::default(),
            period: None,
            watermark: original.time_period_start,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        scope
            .publish(
                vec![empty],
                original.time_period_end,
                bar_type,
                &period,
                &instrument,
                &sender,
            )
            .unwrap();
        assert_eq!(scope.watermark, original.time_period_start);
        assert!(receiver.try_recv().is_err());
        scope
            .publish(
                vec![original.clone()],
                original.time_period_end,
                bar_type,
                &period,
                &instrument,
                &sender,
            )
            .unwrap();

        let DataEvent::Data(Data::Bar(bar)) = receiver.try_recv().unwrap() else {
            panic!("expected bar")
        };

        assert_eq!(
            Some(bar),
            parse::bar(&original, bar_type, &instrument, &period).unwrap()
        );
        assert_eq!(scope.watermark, original.time_period_end);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    #[case(400, true)]
    #[case(401, true)]
    #[case(404, false)]
    #[case(408, false)]
    #[case(429, false)]
    #[case(500, false)]
    #[case(0, true)]
    fn bar_error_classification_changes_only_its_endpoint_schedule(
        #[case] status: u16,
        #[case] suspended: bool,
    ) {
        let instrument = instrument();
        let bar_type = bar_type(&instrument);
        let mut polling = Polling::default();
        polling
            .subscribe_bars(bar_type, Timestamp::from_second(0).unwrap())
            .unwrap();
        polling.subscribe(instrument.id, true, 100).unwrap();
        polling.subscribe_book(instrument.id, true, None).unwrap();
        let scope = polling.subscriptions.get_mut(&instrument.id).unwrap();
        let activity_revision = scope.activity.as_ref().unwrap().schedule.revision;
        let book_revision = scope.book.as_ref().unwrap().schedule.revision;

        let failure = if status == 0 {
            Error::AttemptBudget
        } else {
            Error::Http {
                status,
                retry_after_secs: None,
            }
        };

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        scope
            .complete(
                Feed::Bars(bar_type),
                Err(failure),
                &instrument,
                &sender.into(),
                Duration::from_millis(100),
                100,
            )
            .unwrap();
        assert_eq!(scope.bars[&bar_type].schedule.suspended, suspended);
        assert_eq!(
            scope.bars[&bar_type].watermark,
            Timestamp::from_second(0).unwrap()
        );
        assert_eq!(
            scope.activity.as_ref().unwrap().schedule.revision,
            activity_revision
        );
        assert!(!scope.activity.as_ref().unwrap().schedule.suspended);
        assert_eq!(
            scope.book.as_ref().unwrap().schedule.revision,
            book_revision
        );
        assert!(!scope.book.as_ref().unwrap().schedule.suspended);
        assert!(receiver.try_recv().is_err());
    }

    fn bar_type(instrument: &BinaryOption) -> BarType {
        BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id).as_str())
    }

    fn minute_period() -> OhlcvPeriod {
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../../test_data/periods.json")).unwrap();
        history::period(&periods, (60, 0)).unwrap().clone()
    }

    fn book_timestamp(seconds: u64) -> jiff::Timestamp {
        jiff::Timestamp::from_nanosecond(
            1_791_676_800_000_000_000 + i128::from(seconds) * 1_000_000_000,
        )
        .unwrap()
    }

    fn instrument() -> BinaryOption {
        let market: Market =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
        parse_instrument(&market, UnixNanos::from(0)).unwrap()
    }
}
