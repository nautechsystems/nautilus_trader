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

//! Collection utilities for the data events a client emits onto the live runner channel.

use std::time::Duration;

use nautilus_common::messages::DataEvent;
use nautilus_core::UUID4;

/// Collects the data events received on `rx` within `timeout`.
///
/// While the live runner's thread-local sender keeps the channel open, this waits out the full
/// window and suits absence checks. Prefer [`collect_data_events_until_response`] whenever the
/// events end with a correlated response.
pub async fn drain_data_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    timeout: Duration,
) -> Vec<DataEvent> {
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        events.push(event);
    }
    events
}

/// Collects the data events received on `rx` up to and including the response correlated with
/// `request_id`, then drains any further events available without waiting.
///
/// `timeout` bounds only the wait through the correlated response, so a passing run returns when
/// that response arrives instead of waiting out the window.
///
/// # Panics
///
/// Panics if the channel closes before the correlated response arrives, or if that response does
/// not arrive within `timeout`.
pub async fn collect_data_events_until_response(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    request_id: UUID4,
    timeout: Duration,
) -> Vec<DataEvent> {
    let mut events = Vec::new();
    tokio::time::timeout(timeout, async {
        loop {
            let event = rx.recv().await.expect("data event channel closed");
            let is_correlated_response = matches!(
                &event,
                DataEvent::Response(response) if response.correlation_id() == &request_id
            );
            events.push(event);

            if is_correlated_response {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for data response {request_id}"));

    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    events
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::{DataResponse, data::InstrumentsResponse};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::{ClientId, Venue},
        stubs::TestDefault,
    };
    use rstest::rstest;

    use super::*;
    use crate::common::itch_aapl_equity;

    fn instruments_response(correlation_id: UUID4) -> DataEvent {
        DataEvent::Response(DataResponse::Instruments(InstrumentsResponse::new(
            correlation_id,
            ClientId::test_default(),
            Venue::test_default(),
            Vec::new(),
            None,
            None,
            UnixNanos::default(),
            None,
        )))
    }

    fn correlation_ids(events: &[DataEvent]) -> Vec<Option<UUID4>> {
        events
            .iter()
            .map(|event| match event {
                DataEvent::Response(response) => Some(*response.correlation_id()),
                _ => None,
            })
            .collect()
    }

    #[rstest]
    #[case(0)]
    #[case(3)]
    #[tokio::test(start_paused = true)]
    async fn test_drain_data_events_collects_events_queued_before_deadline(#[case] count: usize) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let request_ids: Vec<UUID4> = (0..count).map(|_| UUID4::new()).collect();
        for request_id in &request_ids {
            tx.send(instruments_response(*request_id)).unwrap();
        }

        let events = drain_data_events(&mut rx, Duration::from_millis(50)).await;

        assert_eq!(
            correlation_ids(&events),
            request_ids.iter().copied().map(Some).collect::<Vec<_>>()
        );
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_drain_data_events_stops_at_the_absolute_deadline() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let first_id = UUID4::new();
        let second_id = UUID4::new();
        let late_id = UUID4::new();

        // The 40ms and 80ms sends land inside the 100ms window, the 160ms send past it
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            tx.send(instruments_response(first_id)).unwrap();
            tokio::time::sleep(Duration::from_millis(40)).await;
            tx.send(instruments_response(second_id)).unwrap();
            tokio::time::sleep(Duration::from_millis(80)).await;
            tx.send(instruments_response(late_id)).unwrap();
        });

        let start = tokio::time::Instant::now();
        let events = drain_data_events(&mut rx, Duration::from_millis(100)).await;

        assert_eq!(
            correlation_ids(&events),
            vec![Some(first_id), Some(second_id)]
        );
        assert_eq!(start.elapsed(), Duration::from_millis(100));
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_drain_data_events_returns_when_channel_closes() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let request_id = UUID4::new();
        tx.send(instruments_response(request_id)).unwrap();
        drop(tx);

        let start = tokio::time::Instant::now();
        let events = drain_data_events(&mut rx, Duration::from_secs(5)).await;

        assert_eq!(correlation_ids(&events), vec![Some(request_id)]);
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_collect_data_events_until_response_returns_at_correlated_response() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let other_id = UUID4::new();
        let request_id = UUID4::new();
        let trailing_id = UUID4::new();
        tx.send(DataEvent::Instrument(itch_aapl_equity())).unwrap();
        tx.send(instruments_response(other_id)).unwrap();
        tx.send(instruments_response(request_id)).unwrap();
        tx.send(instruments_response(trailing_id)).unwrap();

        let start = tokio::time::Instant::now();
        let events =
            collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;

        assert_eq!(
            correlation_ids(&events),
            vec![None, Some(other_id), Some(request_id), Some(trailing_id)]
        );
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "timed out waiting for data response")]
    async fn test_collect_data_events_until_response_panics_without_correlated_response() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(instruments_response(UUID4::new())).unwrap();

        collect_data_events_until_response(&mut rx, UUID4::new(), Duration::from_millis(50)).await;
    }

    #[rstest]
    #[tokio::test]
    #[should_panic(expected = "data event channel closed")]
    async fn test_collect_data_events_until_response_panics_when_channel_closes() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        drop(tx);

        collect_data_events_until_response(&mut rx, UUID4::new(), Duration::from_secs(5)).await;
    }
}
