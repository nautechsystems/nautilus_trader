// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
//
use std::collections::HashMap;

use hyper::Method;
use nautilus_network::HttpClient;

// Testing with nginx docker container
// `docker run --publish 8080:80 nginx`
#[tokio::main]
async fn main() {
    let client = HttpClient::default();
    let mut success = 0;
    for _ in 0..100_000 {
        if let Ok(resp) = client
            .send_request(
                Method::GET,
                "http://localhost:8080".to_string(),
                HashMap::new(),
            )
            .await
        {
            if resp.status == 200 {
                success += 1;
            }
        }
    }

    println!("Successful requests: {success}");
}
