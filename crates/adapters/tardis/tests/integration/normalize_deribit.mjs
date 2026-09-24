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

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const { normalizeBookChanges } = await import(
  pathToFileURL(resolve(process.argv[2], "dist/index.js"))
);
const messages = JSON.parse(readFileSync(process.argv[3], "utf8"));
const timestamp = new Date("2024-09-24T00:00:00.000Z");
const mapper = normalizeBookChanges("deribit", timestamp);

for (const message of messages) {
  for (const change of mapper.map(message, timestamp)) {
    process.stdout.write(JSON.stringify(change) + "\n");
  }
}
