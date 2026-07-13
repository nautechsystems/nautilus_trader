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

#![cfg(feature = "high-precision")]

use nautilus_core::UnixNanos;
use nautilus_model::data::{Data, HasTsInit, QuoteTick};
use nautilus_persistence::backend::session::DataBackendSession;
use nautilus_testkit::common::ensure_histdata_eurusd_quotes_parquet;
use rstest::rstest;

#[rstest]
fn test_pinned_quotes_through_direct_session() {
    let filepath = ensure_histdata_eurusd_quotes_parquet();
    let mut session = DataBackendSession::new(1_000);
    session
        .add_file::<QuoteTick>(
            "quotes",
            filepath
                .to_str()
                .expect("test data path must be valid UTF-8"),
            Some("SELECT * FROM quotes ORDER BY ts_init LIMIT 20000"),
            None,
        )
        .unwrap();
    let data: Vec<Data> = session.get_query_result().collect();

    assert_eq!(data.len(), 20_000);
    assert!(data.iter().all(|item| matches!(item, Data::Quote(_))));
    assert!(
        data.windows(2)
            .all(|pair| pair[0].ts_init() <= pair[1].ts_init())
    );
    assert_eq!(
        data.first().expect("query must return quotes").ts_init(),
        UnixNanos::from(1_577_898_010_447_000_000),
    );
    assert_eq!(
        data.last().expect("query must return quotes").ts_init(),
        UnixNanos::from(1_577_934_143_122_000_000),
    );
}
