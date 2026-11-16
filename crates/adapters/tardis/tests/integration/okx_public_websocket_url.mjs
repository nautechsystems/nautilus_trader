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

import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const { createRealTimeFeed } = await import(
  pathToFileURL(resolve(process.argv[2], "dist/index.js"))
);
const exchanges = ["okex", "okex-futures", "okex-swap", "okex-options"];
const filters = [{ channel: "trades", symbols: ["BTC-USDT"] }];
const expectedUrl = "wss://ws.okx.com/ws/v5/public";

for (const exchange of exchanges) {
  const override = `WSS_URL_${exchange.toUpperCase().replace(/-/g, "_")}`;
  delete process.env[override];

  const feed = createRealTimeFeed(exchange, filters);
  const connections = [...feed._getRealTimeFeeds(exchange, filters)];

  if (connections.length !== 1) {
    throw new Error(
      `${exchange} public WebSocket connection count was ${connections.length}; expected 1`,
    );
  }

  const actualUrl = await connections[0].getWebSocketUrl();

  if (actualUrl !== expectedUrl) {
    throw new Error(
      `${exchange} public WebSocket URL was ${actualUrl}; expected ${expectedUrl}`,
    );
  }
}
