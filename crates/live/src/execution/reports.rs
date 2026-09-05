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

use nautilus_common::messages::execution::GenerateOrderStatusReports;
use nautilus_model::reports::OrderStatusReport;

/// Retains order status reports matching the command's status and time filters.
///
/// Open-only requests include both open and in-flight reports. The inclusive `start` and `end`
/// bounds apply only to closed reports.
pub fn retain_order_status_reports(
    reports: &mut Vec<OrderStatusReport>,
    command: &GenerateOrderStatusReports,
) {
    reports.retain(|report| {
        let status = report.order_status;
        let matches_open = !command.open_only || status.is_open() || status.is_inflight();
        let matches_time = !status.is_closed()
            || (command.start.is_none_or(|start| report.ts_last >= start)
                && command.end.is_none_or(|end| report.ts_last <= end));

        matches_open && matches_time
    });
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::execution::GenerateOrderStatusReports;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
        identifiers::{AccountId, InstrumentId, VenueOrderId},
        reports::OrderStatusReport,
        types::Quantity,
    };
    use rstest::rstest;

    use super::retain_order_status_reports;

    #[rstest]
    #[case::accepted(OrderStatus::Accepted, true)]
    #[case::submitted(OrderStatus::Submitted, true)]
    #[case::initialized(OrderStatus::Initialized, false)]
    #[case::emulated(OrderStatus::Emulated, false)]
    #[case::released(OrderStatus::Released, false)]
    #[case::filled(OrderStatus::Filled, false)]
    fn test_retain_order_status_reports_filters_open_and_inflight(
        #[case] status: OrderStatus,
        #[case] expected_retained: bool,
    ) {
        let command = order_status_reports_command(true, None, None);
        let mut reports = vec![order_status_report(status, UnixNanos::from(5))];

        retain_order_status_reports(&mut reports, &command);

        assert_eq!(reports.len(), usize::from(expected_retained));
    }

    #[rstest]
    #[case::local_before(OrderStatus::Initialized, 9, true)]
    #[case::open_before(OrderStatus::Accepted, 9, true)]
    #[case::inflight_after(OrderStatus::Submitted, 21, true)]
    #[case::closed_before(OrderStatus::Filled, 9, false)]
    #[case::closed_at_start(OrderStatus::Filled, 10, true)]
    #[case::closed_at_end(OrderStatus::Filled, 20, true)]
    #[case::closed_after(OrderStatus::Filled, 21, false)]
    fn test_retain_order_status_reports_filters_only_closed_reports_by_time(
        #[case] status: OrderStatus,
        #[case] ts_last: u64,
        #[case] expected_retained: bool,
    ) {
        let command = order_status_reports_command(
            false,
            Some(UnixNanos::from(10)),
            Some(UnixNanos::from(20)),
        );
        let mut reports = vec![order_status_report(status, UnixNanos::from(ts_last))];

        retain_order_status_reports(&mut reports, &command);

        assert_eq!(reports.len(), usize::from(expected_retained));
    }

    fn order_status_reports_command(
        open_only: bool,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> GenerateOrderStatusReports {
        GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            open_only,
            None,
            start,
            end,
            None,
            None,
        )
    }

    fn order_status_report(status: OrderStatus, ts_last: UnixNanos) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUD/USD.SIM"),
            None,
            VenueOrderId::from("ORDER-001"),
            Some(OrderSide::Buy),
            OrderType::Limit,
            TimeInForce::Gtc,
            status,
            Quantity::from("1"),
            Quantity::zero(0),
            UnixNanos::from(1),
            ts_last,
            UnixNanos::from(2),
            None,
        )
    }
}
