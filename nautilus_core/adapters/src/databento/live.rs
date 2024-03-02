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

use std::{collections::HashMap, ffi::CStr, sync::mpsc::Receiver};

use anyhow::Result;
use databento::{
    dbn::{PitSymbolMap, Record, SymbolIndex, VersionUpgradePolicy},
    live::Subscription,
};
use indexmap::IndexMap;
use log::{error, info};
use nautilus_core::{
    python::{to_pyruntime_err, to_pyvalue_err},
    time::{get_atomic_clock_realtime, AtomicTime},
};
use nautilus_model::{
    data::{
        delta::OrderBookDelta,
        deltas::{OrderBookDeltas, OrderBookDeltas_API},
        Data,
    },
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    instruments::Instrument,
};
use tokio::sync::mpsc::Sender;
use ustr::Ustr;

use crate::databento::{
    decode::{decode_instrument_def_msg, decode_record},
    types::PublisherId,
};

pub struct StartCommand {
    pub replay: bool,
}

pub enum LiveCommand {
    Subscribe(Subscription),
    UpdateGlbx(HashMap<Symbol, Venue>),
    Start(StartCommand),
    Close,
}

#[allow(clippy::large_enum_variant)] // TODO: Optimize this (largest variant 1096 vs 80 bytes)
pub enum LiveMessage {
    Data(Data),
    Instrument(Box<dyn Instrument>),
    Error(databento::Error),
}

pub struct DatabentoFeedHandler {
    key: String,
    dataset: String,
    rx: Receiver<LiveCommand>,
    tx: Sender<LiveMessage>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
    glbx_exchange_map: HashMap<Symbol, Venue>,
}

impl DatabentoFeedHandler {
    #[must_use]
    pub fn new(
        key: String,
        dataset: String,
        rx: Receiver<LiveCommand>,
        tx: Sender<LiveMessage>,
        publisher_venue_map: IndexMap<PublisherId, Venue>,
        glbx_exchange_map: HashMap<Symbol, Venue>,
    ) -> Self {
        Self {
            key,
            dataset,
            rx,
            tx,
            publisher_venue_map,
            glbx_exchange_map,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let clock = get_atomic_clock_realtime();
        let mut symbol_map = PitSymbolMap::new();
        let mut instrument_id_map: HashMap<u32, InstrumentId> = HashMap::new();

        let mut buffering_start = None;
        let mut buffered_deltas: HashMap<InstrumentId, Vec<OrderBookDelta>> = HashMap::new();
        let mut deltas_count = 0_u64;

        let mut client = databento::LiveClient::builder()
            .key(self.key.clone())?
            .dataset(self.dataset.clone())
            .upgrade_policy(VersionUpgradePolicy::Upgrade)
            .build()
            .await?;

        let mut running = false;

        loop {
            // Check for any commands received
            if let Ok(cmd) = self.rx.recv() {
                match cmd {
                    LiveCommand::Subscribe(sub) => {
                        client.subscribe(&sub).await?;
                    }
                    LiveCommand::UpdateGlbx(map) => self.glbx_exchange_map = map,
                    LiveCommand::Start(cmd) => {
                        buffering_start = match cmd.replay {
                            true => Some(clock.get_time_ns()),
                            false => None,
                        };
                        client.start().await.map_err(to_pyruntime_err)?;
                        running = true;
                    }
                    LiveCommand::Close => {
                        client.close().await.map_err(to_pyruntime_err)?;
                        return Ok(());
                    }
                }
            }

            if !running {
                continue;
            };

            let record = match client.next_record().await {
                Ok(Some(record)) => record,
                Ok(None) => break, // Session ended normally
                Err(e) => {
                    // Fail the session entirely for now. Consider refining
                    // this strategy to handle specific errors more gracefully.
                    self.tx
                        .send(LiveMessage::Error(e))
                        .await
                        .expect("Error sending ");
                    break;
                }
            };

            if let Some(msg) = record.get::<dbn::ErrorMsg>() {
                handle_error_msg(msg);
            } else if let Some(msg) = record.get::<dbn::SystemMsg>() {
                handle_system_msg(msg);
            } else if let Some(msg) = record.get::<dbn::SymbolMappingMsg>() {
                // Remove instrument ID index as the raw symbol may have changed
                instrument_id_map.remove(&msg.hd.instrument_id);
                handle_symbol_mapping_msg(msg, &mut symbol_map);
            } else if let Some(msg) = record.get::<dbn::InstrumentDefMsg>() {
                let inst = handle_instrument_def_msg(
                    msg,
                    &self.publisher_venue_map,
                    &self.glbx_exchange_map,
                    &mut instrument_id_map,
                    clock,
                )?;
                self.tx.send(LiveMessage::Instrument(inst)).await.unwrap();
            } else {
                let (mut data1, data2) = handle_record(
                    record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.glbx_exchange_map,
                    &mut instrument_id_map,
                    clock,
                )?;

                if let Some(msg) = record.get::<dbn::MboMsg>() {
                    // SAFETY: An MBO message will always produce a delta
                    if let Data::Delta(delta) = data1.clone().unwrap() {
                        let buffer = buffered_deltas.entry(delta.instrument_id).or_default();
                        buffer.push(delta);

                        // TODO: Temporary for debugging
                        deltas_count += 1;
                        println!(
                            "Buffering delta: {} {} {:?} flags={}",
                            deltas_count, delta.ts_event, buffering_start, msg.flags,
                        );

                        // Check if last message in the packet
                        if msg.flags & dbn::flags::LAST == 0 {
                            continue; // NOT last message
                        }

                        // Check if snapshot
                        if msg.flags & dbn::flags::SNAPSHOT != 0 {
                            continue; // Buffer snapshot
                        }

                        // Check if buffering a replay
                        if let Some(start_ns) = buffering_start {
                            if delta.ts_event <= start_ns {
                                continue; // Continue buffering replay
                            }
                            buffering_start = None;
                        }

                        // SAFETY: We can guarantee a deltas vec exists
                        let buffer = buffered_deltas.remove(&delta.instrument_id).unwrap();
                        let deltas = OrderBookDeltas::new(delta.instrument_id, buffer);
                        let deltas = OrderBookDeltas_API::new(deltas);
                        data1 = Some(Data::Deltas(deltas));
                    }
                };

                if let Some(data) = data1 {
                    match self.tx.send(LiveMessage::Data(data)).await {
                        Ok(()) => {}
                        Err(e) => eprintln!("{e:?}"), // Print stderr for now
                    }
                }

                if let Some(data) = data2 {
                    match self.tx.send(LiveMessage::Data(data)).await {
                        Ok(()) => {}
                        Err(e) => eprintln!("{e:?}"), // Print stderr for now
                    }
                }
            };
        }

        Ok(())
    }
}

fn handle_error_msg(msg: &dbn::ErrorMsg) {
    eprintln!("{msg:?}"); // TODO: Just print stderr for now
    error!("{:?}", msg);
}

fn handle_system_msg(msg: &dbn::SystemMsg) {
    println!("{msg:?}"); // TODO: Just print stdout for now
    info!("{:?}", msg);
}

fn handle_symbol_mapping_msg(msg: &dbn::SymbolMappingMsg, symbol_map: &mut PitSymbolMap) {
    symbol_map
        .on_symbol_mapping(msg)
        .unwrap_or_else(|_| panic!("Error updating `symbol_map` with {msg:?}"));
}

fn update_instrument_id_map(
    header: &dbn::RecordHeader,
    raw_symbol: &str,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    glbx_exchange_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
) -> InstrumentId {
    // Check if instrument ID is already in the map
    if let Some(&instrument_id) = instrument_id_map.get(&header.instrument_id) {
        return instrument_id;
    }

    let symbol = Symbol {
        value: Ustr::from(raw_symbol),
    };

    let publisher_id = header.publisher_id;
    let venue = match glbx_exchange_map.get(&symbol) {
        Some(venue) => venue,
        None => publisher_venue_map
            .get(&publisher_id)
            .unwrap_or_else(|| panic!("No venue found for `publisher_id` {publisher_id}")),
    };
    let instrument_id = InstrumentId::new(symbol, *venue);

    instrument_id_map.insert(header.instrument_id, instrument_id);
    instrument_id
}

fn handle_instrument_def_msg(
    msg: &dbn::InstrumentDefMsg,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    glbx_exchange_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    clock: &AtomicTime,
) -> Result<Box<dyn Instrument>> {
    let c_str: &CStr = unsafe { CStr::from_ptr(msg.raw_symbol.as_ptr()) };
    let raw_symbol: &str = c_str.to_str().map_err(to_pyvalue_err)?;

    let instrument_id = update_instrument_id_map(
        msg.header(),
        raw_symbol,
        publisher_venue_map,
        glbx_exchange_map,
        instrument_id_map,
    );

    let ts_init = clock.get_time_ns();
    decode_instrument_def_msg(msg, instrument_id, ts_init)
}

fn handle_record(
    rec_ref: dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    glbx_exchange_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    clock: &AtomicTime,
) -> Result<(Option<Data>, Option<Data>)> {
    let raw_symbol = symbol_map
        .get_for_rec(&rec_ref)
        .expect("Cannot resolve `raw_symbol` from `symbol_map`");

    let instrument_id = update_instrument_id_map(
        rec_ref.header(),
        raw_symbol,
        publisher_venue_map,
        glbx_exchange_map,
        instrument_id_map,
    );

    let price_precision = 2; // Hard coded for now
    let ts_init = clock.get_time_ns();

    decode_record(
        &rec_ref,
        instrument_id,
        price_precision,
        Some(ts_init),
        true, // Always include trades
    )
}
