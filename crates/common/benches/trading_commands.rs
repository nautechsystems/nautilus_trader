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

//! Measures synchronous trading command enqueue, routing, and deferred-child processing.
//!
//! Independent ingress stays root-free until it produces callback work or another command.
//! The scoped workload uses a data-command handler to share a root without exposing private
//! callback primitives. Inputs are constructed before timing; message moves and drops are timed.

use std::{
    cell::{Cell, RefCell},
    hint::black_box,
    rc::Rc,
};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    messages::{
        data::{DataCommand, SubscribeCommand, SubscribeQuotes},
        execution::{QueryAccount, TradingCommand},
    },
    msgbus::{self, MessagingSwitchboard, TypedIntoHandler},
    runner::{
        DataCommandSender, SyncDataCommandSender, SyncTradingCommandSender, TradingCommandMessage,
        TradingCommandSender, capture_trading_cmd, data_cmd_queue_is_empty, drain_data_cmd_queue,
        drain_trading_cmd_queue, trading_cmd_queue_is_empty,
    },
};
use nautilus_core::UUID4;

fn bench_trading_commands(c: &mut Criterion) {
    let mut group = c.benchmark_group("trading_commands");
    let endpoint = MessagingSwitchboard::exec_engine_execute();
    let received = Rc::new(Cell::new(0usize));
    let observed = received.clone();
    msgbus::register_trading_command_endpoint(
        endpoint,
        TypedIntoHandler::from(move |command| {
            black_box(command);
            observed.set(observed.get() + 1);
        }),
    );

    for count in [1, 64] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("independent", count),
            &count,
            |b, &count| {
                let run = |commands: &mut Vec<TradingCommandMessage>| {
                    for command in commands.drain(..) {
                        SyncTradingCommandSender.execute(black_box(command));
                    }

                    drain_trading_cmd_queue();
                };

                let before = received.get();
                run(&mut (0..count).map(|_| trading_command()).collect());
                assert_eq!(received.get() - before, count);
                assert!(trading_cmd_queue_is_empty());
                b.iter_batched_ref(
                    || (0..count).map(|_| trading_command()).collect(),
                    run,
                    BatchSize::SmallInput,
                );
            },
        );
    }

    let commands = Rc::new(RefCell::new(Vec::new()));
    let queued = commands.clone();
    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_execute(),
        TypedIntoHandler::from(move |_| {
            for command in queued.borrow_mut().drain(..) {
                SyncTradingCommandSender.execute(command);
            }
        }),
    );

    group.throughput(Throughput::Elements(64));
    group.bench_function("shared_root/64", |b| {
        let run = |input: &mut Option<DataCommand>| {
            SyncDataCommandSender.execute(black_box(input.take().unwrap()));
            drain_data_cmd_queue();
            drain_trading_cmd_queue();
        };

        let setup = || {
            commands
                .borrow_mut()
                .extend((0..64).map(|_| trading_command()));
            Some(data_command())
        };

        let before = received.get();
        run(&mut setup());
        assert_eq!(received.get() - before, 64);
        assert!(data_cmd_queue_is_empty());
        assert!(trading_cmd_queue_is_empty());
        b.iter_batched_ref(setup, run, BatchSize::PerIteration);
    });

    let parent = MessagingSwitchboard::risk_engine_execute();

    for count in [1, 64] {
        let children = Rc::new(RefCell::new(Vec::new()));
        let queued = children.clone();
        msgbus::register_trading_command_endpoint(
            parent,
            TypedIntoHandler::from(move |_| {
                for command in queued.borrow_mut().drain(..) {
                    capture_trading_cmd(command);
                }
            }),
        );

        group.throughput(Throughput::Elements(count as u64 + 1));
        group.bench_with_input(BenchmarkId::new("children", count), &count, |b, &count| {
            let run = |input: &mut Option<TradingCommandMessage>| {
                SyncTradingCommandSender.execute(black_box(input.take().unwrap()));
                drain_trading_cmd_queue();
            };

            let setup = || {
                children
                    .borrow_mut()
                    .extend((0..count).map(|_| trading_command()));
                Some(TradingCommandMessage::new(
                    parent,
                    trading_command().command().clone(),
                ))
            };

            let before = received.get();
            run(&mut setup());
            assert_eq!(received.get() - before, count);
            assert!(trading_cmd_queue_is_empty());
            b.iter_batched_ref(setup, run, BatchSize::PerIteration);
        });
    }

    group.finish();
}

fn trading_command() -> TradingCommandMessage {
    TradingCommandMessage::new(
        MessagingSwitchboard::exec_engine_execute(),
        TradingCommand::QueryAccount(QueryAccount::new(
            "BENCH-001".into(),
            None,
            "SIM-001".into(),
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            1.into(),
            None,
            None,
        )),
    )
}

fn data_command() -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        "AUD/USD.SIM".into(),
        Some("SIM".into()),
        None,
        UUID4::from("00000000-0000-4000-8000-000000000002"),
        2.into(),
        None,
        None,
    )))
}

criterion_group!(benches, bench_trading_commands);
criterion_main!(benches);
