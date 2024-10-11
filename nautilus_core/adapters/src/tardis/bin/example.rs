// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

// use chrono::NaiveDate;
// use futures_util::{pin_mut, StreamExt};
// use nautilus_adapters::tardis::{
//     enums::Exchange,
//     machine::{ReplayNormalizedRequestOptions, TardisClient},
// };

#[tokio::main]
async fn main() {
    // tracing_subscriber::fmt()
    //     .with_max_level(tracing::Level::TRACE)
    //     .init();
    //
    // let client = TardisClient::new(std::env::var("TARDIS_MACHINE_WS_URL").unwrap());
    //
    // let stream = client
    //     .replay_normalized(vec![ReplayNormalizedRequestOptions {
    //         exchange: Exchange::Bitmex,
    //         symbols: Some(vec!["XBTUSD".to_string()]),
    //         from: NaiveDate::from_ymd_opt(2019, 10, 1).unwrap(),
    //         to: NaiveDate::from_ymd_opt(2019, 10, 2).unwrap(),
    //         data_types: vec!["book_change".to_string()],
    //         with_disconnect_messages: Some(true),
    //     }])
    //     .await
    //     .unwrap();
    //
    // pin_mut!(stream);
    //
    // while let Some(msg) = stream.next().await {
    //     println!("Received trade bar: {:?}", msg);
    // }
}
