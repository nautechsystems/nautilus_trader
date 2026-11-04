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

#![cfg(all(feature = "defi", any(feature = "redis", feature = "postgres")))]

use std::{process::Command, sync::Arc};

use nautilus_common::cache::{Cache, CacheSnapshotRef, database::CacheDatabaseAdapter};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    defi::{
        Blockchain, Chain, Token,
        currency::{self, TokenCurrency},
    },
    enums::{CurrencyType, OmsType, OrderSide, OrderType},
    events::{OrderSnapshot, PositionSnapshot, order::spec::OrderFilledSpec},
    identifiers::{ClientId, InstrumentId, PositionId, Symbol, TradeId, TraderId},
    instruments::{CurrencyPair, Instrument, InstrumentAny, stubs::currency_pair_ethusdt},
    orders::{Order, builder::OrderTestBuilder},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};

const READER_CASE: &str = "NAUTILUS_DEFI_CURRENCY_READER_CASE";

fn definitions(chain_id: u32) -> [Token; 2] {
    let chain = Arc::new(Chain::new(Blockchain::Ethereum, chain_id));
    [
        Token::new(
            chain.clone(),
            "0x1122334455667788990011223344556677889900"
                .parse()
                .unwrap(),
            "First token".into(),
            format!("BASE{chain_id}"),
            17,
        ),
        Token::new(
            chain,
            "0x9988776655443322110099887766554433221100"
                .parse()
                .unwrap(),
            "Second token".into(),
            format!("QUOTE{chain_id}"),
            18,
        ),
    ]
}

fn instrument(chain_id: u32, register: bool) -> InstrumentAny {
    let tokens = definitions(chain_id);

    let currencies = tokens.map(|token| {
        if register {
            token.currency().unwrap()
        } else {
            let code = TokenCurrency::from(&token).code();

            Currency::new(
                code.as_str(),
                token.decimals,
                0,
                token.name.as_str(),
                CurrencyType::Crypto,
            )
        }
    });

    CurrencyPair::builder()
        .instrument_id(InstrumentId::from("ABCABC.DEFI"))
        .raw_symbol(Symbol::from("ABCABC"))
        .base_currency(currencies[0])
        .quote_currency(currencies[1])
        .price_precision(16)
        .size_precision(16)
        .price_increment(Price::from("0.0000000000000001"))
        .size_increment(Quantity::from("0.0000000000000001"))
        .ts_event(UnixNanos::from(123))
        .ts_init(UnixNanos::from(456))
        .build()
        .unwrap()
        .into_any()
}

fn renamed_token(chain_id: u32, suffix: &str) -> Token {
    let mut token = definitions(chain_id)[0].clone();
    token.symbol = format!("RENAMED{chain_id}{suffix}");
    token
}

fn legacy_currency() -> Currency {
    Currency::new(
        "LEGACY@ETHEREUM",
        3,
        0,
        "Legacy custom currency",
        CurrencyType::Crypto,
    )
}

fn assert_legacy_definition() {
    let legacy = legacy_currency();
    let registered = Currency::try_from_str(legacy.code.as_str()).unwrap();
    assert_eq!(registered.precision, 3);
    assert_eq!(registered.name, "Legacy custom currency");
    assert_eq!(currency::lookup(legacy.code), None);
    let mut colliding = definitions(823_105)[0].clone();
    colliding.decimals = 3;
    colliding.symbol = "LEGACY".into();
    assert!(colliding.currency().is_err());
}

fn conflict_instrument(chain_id: u32, reversed: bool) -> InstrumentAny {
    let mut pair = match instrument(chain_id, false) {
        InstrumentAny::CurrencyPair(pair) => pair,
        _ => unreachable!(),
    };

    let mut tokens = definitions(chain_id);

    tokens[0].decimals = if reversed { 18 } else { 17 };
    pair.base_currency = tokens[0].currency().unwrap();
    pair.quote_currency = tokens[1].currency().unwrap();
    pair.into_any()
}

fn closed_position(instrument: &InstrumentAny) -> Position {
    let mut fill = OrderFilledSpec::builder().event_id(UUID4::new()).build();
    fill.instrument_id = instrument.id();
    fill.position_id = Some(PositionId::from("DEFI-HISTORY"));
    fill.currency = instrument.quote_currency();
    fill.order_side = OrderSide::Buy;
    fill.last_qty = Quantity::from("1.00000");
    fill.last_px = Price::from("100.00");
    fill.commission = Some(Money::from_decimal_exact("1".parse().unwrap(), fill.currency).unwrap());
    let mut position = Position::new_checked(instrument, fill.clone()).unwrap();
    fill.event_id = UUID4::new();
    fill.trade_id = TradeId::from("HISTORY-CLOSE");
    fill.order_side = OrderSide::Sell;
    fill.last_px = Price::from("110.00");
    fill.ts_event = UnixNanos::from(2);
    position.try_apply(&fill).unwrap();
    assert!(position.is_closed());
    assert_eq!(
        position.realized_pnl.unwrap().as_decimal(),
        "8".parse().unwrap()
    );
    position
}

fn position_archive(mode: &str, legacy: &InstrumentAny) -> Vec<u8> {
    let unrelated = currency_pair_ethusdt().into_any();

    let position = closed_position(if mode == "archive" {
        legacy
    } else {
        &unrelated
    });

    serde_json::to_vec(&position).unwrap()
}

fn legacy_instrument() -> InstrumentAny {
    let mut pair = currency_pair_ethusdt();
    pair.id = InstrumentId::from("ABCABC.DEFI");
    pair.into_any()
}

fn reversed_instrument(instrument: &InstrumentAny) -> InstrumentAny {
    let InstrumentAny::CurrencyPair(mut pair) = instrument.clone() else {
        unreachable!()
    };

    std::mem::swap(&mut pair.base_currency, &mut pair.quote_currency);
    pair.into_any()
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn cached_token_identities_cannot_change(#[case] reverse: bool) {
    let pair = instrument(823_123, true);
    let mut cache = Cache::default();
    cache.add_instrument(pair.clone()).unwrap();
    let replacement = if reverse {
        reversed_instrument(&pair)
    } else {
        legacy_instrument()
    };

    let error = cache.add_instrument(replacement).unwrap_err();

    assert_eq!(
        error.to_string(),
        "DeFi currency integrity: Cannot change a token instrument's currency identities"
    );
    assert_instrument(cache.instrument(&pair.id()).unwrap(), &pair, 823_123);
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
fn purged_instrument_retains_position_history_guard(#[case] archived: bool) {
    let legacy = legacy_instrument();
    let mut cache = Cache::default();
    cache.add_instrument(legacy.clone()).unwrap();
    let position = closed_position(&legacy);
    cache.add_position(&position, OmsType::Netting).unwrap();
    if archived {
        cache.snapshot_position_encoded(&position).unwrap();
        cache.purge_position(position.id, false).unwrap();
    }

    cache.purge_instrument(legacy.id());

    let error = cache.add_instrument(instrument(823_122, true)).unwrap_err();

    assert!(error.to_string().contains("position history"), "{error:#}");
    assert_eq!(cache.instrument(&legacy.id()), None);
}

#[rstest::rstest]
fn purged_instrument_retains_order_guard() {
    let legacy = legacy_instrument();
    let mut cache = Cache::default();
    cache.add_instrument(legacy.clone()).unwrap();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(legacy.id())
        .quantity(Quantity::from("1.00000"))
        .build();
    cache.add_order(order, None, None, false).unwrap();
    cache.purge_instrument_skip_order_guard(legacy.id());

    let error = cache.add_instrument(instrument(823_134, true)).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("Instrument {} has nonterminal orders", legacy.id())
    );
    assert_eq!(cache.instrument(&legacy.id()), None);
}

fn replacement_failure(mode: &str) -> &'static str {
    match mode {
        "history" | "archive" | "unrelated_archive" => "position history",
        "orders" | "order_snapshot" => "nonterminal order",
        "intents" => "active swap intents",
        _ => "No authoritative execution-store check",
    }
}

fn assert_instrument(actual: &InstrumentAny, expected: &InstrumentAny, chain_id: u32) {
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );

    for ((actual, expected), token) in [
        (
            actual.base_currency().unwrap(),
            expected.base_currency().unwrap(),
        ),
        (actual.quote_currency(), expected.quote_currency()),
    ]
    .into_iter()
    .zip(definitions(chain_id))
    {
        assert_eq!(actual.code, expected.code);
        assert_eq!(actual.precision, expected.precision);
        assert_eq!(actual.iso4217, expected.iso4217);
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.currency_type, expected.currency_type);
        let (registered, metadata) = currency::lookup(actual.code).unwrap();
        assert_eq!(registered.precision, expected.precision);
        assert_eq!(metadata.code(), expected.code.as_str());
        assert_eq!(metadata, TokenCurrency::from(&token));
        assert_eq!(metadata.code(), expected.code.as_str());
    }

    let base = actual.base_currency().unwrap();
    let quote = actual.quote_currency();
    let quantum =
        rust_decimal::Decimal::new(1, u32::from(expected.base_currency().unwrap().precision));
    assert_eq!(Money::from_decimal_exact(quantum, base).unwrap().raw(), 1);
    assert_eq!(
        Money::from_decimal_exact("0.000000000000000001".parse().unwrap(), quote)
            .unwrap()
            .raw(),
        1
    );
}

fn assert_token(actual: Currency, expected: &Token) {
    let metadata = TokenCurrency::from(expected);
    assert_eq!(actual.code.as_str(), metadata.code());
    assert_eq!(actual.precision, expected.decimals);
    assert_eq!(actual.iso4217, 0);
    assert_eq!(actual.name.as_str(), expected.name);
    assert_eq!(actual.currency_type, CurrencyType::Crypto);
    assert_eq!(currency::lookup(actual.code).unwrap().1, metadata);
}

fn reader(test: &str, case: &str, marker: &str) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    if test.starts_with("postgres_tests::") {
        command.env("PGOPTIONS", format!("-c search_path={case}"));
    }

    let output = command
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(READER_CASE, case)
        .env("NAUTILUS_WORKER_THREADS", "2")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Reader failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(marker));
}

fn concurrent_writers(test: &str, case: &str, postgres: bool) -> [std::process::Output; 2] {
    let children = ["17", "18"].map(|precision| {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(READER_CASE, case)
            .env("NAUTILUS_DEFI_WRITER_CASE", precision)
            .env("NAUTILUS_WORKER_THREADS", "2");

        if postgres {
            command.env("PGOPTIONS", format!("-c search_path={case}"));
        }

        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    });

    children.map(|child| child.wait_with_output().unwrap())
}

fn assert_writer_results(outputs: [std::process::Output; 2]) {
    let mut committed = 0;

    for output in outputs {
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        committed +=
            usize::from(String::from_utf8_lossy(&output.stdout).contains("WRITER_COMMITTED"));
    }

    assert_eq!(committed, 1);
}

#[cfg(feature = "redis")]
mod redis_tests {
    use nautilus_common::{cache::CacheConfig, enums::SerializationEncoding};
    use nautilus_infrastructure::redis::{
        cache::{RedisCacheConfig, RedisCacheDatabase, RedisCacheDatabaseAdapter},
        queries::DatabaseQueries,
    };
    use nautilus_model::identifiers::TraderId;
    use redis::AsyncCommands;
    use rstest::rstest;

    use super::*;

    async fn adapter(key: &str, encoding: SerializationEncoding) -> RedisCacheDatabaseAdapter {
        RedisCacheDatabaseAdapter {
            database: RedisCacheDatabase::new(
                TraderId::from(key),
                UUID4::new(),
                CacheConfig {
                    encoding,
                    buffer_interval_ms: Some(50),
                    ..Default::default()
                },
                RedisCacheConfig {
                    connection_timeout: 2,
                    number_of_retries: 0,
                    ..Default::default()
                },
            )
            .await
            .unwrap(),
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn renamed_contract_is_rejected(#[case] old_record: bool) {
        let child = std::env::var(READER_CASE).ok();
        let key = child
            .clone()
            .unwrap_or_else(|| format!("DEFI-{}", UUID4::new()));
        let mut database = adapter(&key, SerializationEncoding::Json).await;
        let original = definitions(823_135)[0].clone();
        let old_code = TokenCurrency::from(&original).code();
        let mut con = database.database.con.clone();
        let trader_key = database.database.trader_key.clone();

        if child.is_some() {
            let metadata = TokenCurrency::from(&original);

            let untagged = Currency::new(
                metadata.code().as_str(),
                6,
                0,
                "Stale legacy definition",
                CurrencyType::Crypto,
            );
            let legacy_error = database.add_currency(&untagged).unwrap_err();
            assert!(
                legacy_error.to_string().contains("token code"),
                "{legacy_error:#}"
            );
            let renamed = renamed_token(823_135, "").currency().unwrap();
            let error = database.add_currency(&renamed).unwrap_err();
            let saved: Option<Vec<u8>> = con
                .get(format!("{trader_key}:currencies:{}", renamed.code))
                .await
                .unwrap();
            database.close().unwrap();
            assert!(
                error
                    .to_string()
                    .contains(&format!("already persisted as {old_code}")),
                "{error:#}"
            );
            assert_eq!(saved, None);
            println!("RENAMED_WRITE_REJECTED");
            return;
        }

        database
            .add_currency(&original.currency().unwrap())
            .unwrap();

        if old_record {
            let binding = format!(
                "{trader_key}:defi:contracts:{}:{:#x}",
                original.chain.chain_id, original.address
            );
            let _: () = con.del(binding).await.unwrap();
        }

        let test = if old_record {
            "redis_tests::renamed_contract_is_rejected::case_2"
        } else {
            "redis_tests::renamed_contract_is_rejected::case_1"
        };

        reader(test, &key, "RENAMED_WRITE_REJECTED");
        let currencies =
            DatabaseQueries::load_currencies(&con, &trader_key, SerializationEncoding::Json)
                .await
                .unwrap();
        let binding: Option<String> = con
            .get(format!(
                "{trader_key}:defi:contracts:{}:{:#x}",
                original.chain.chain_id, original.address
            ))
            .await
            .unwrap();
        assert_eq!(binding, (!old_record).then_some(old_code));
        let keys: Vec<String> = con.keys(format!("{trader_key}:*")).await.unwrap();
        let _: () = con.del(keys).await.unwrap();
        database.close().unwrap();
        assert_eq!(currencies.len(), 1);
        assert_token(*currencies.values().next().unwrap(), &original);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn independent_writers_reject_symbol_aliases() {
        let child = std::env::var(READER_CASE).ok();
        let key = child
            .clone()
            .unwrap_or_else(|| format!("DEFI-{}", UUID4::new()));
        let mut database = adapter(&key, SerializationEncoding::Json).await;

        if child.is_some() {
            let suffix = std::env::var("NAUTILUS_DEFI_WRITER_CASE").unwrap();
            let token = renamed_token(823_136, &suffix);
            match database.add_currency(&token.currency().unwrap()) {
                Ok(()) => println!("WRITER_COMMITTED"),
                Err(e) => assert!(e.to_string().contains("already persisted as"), "{e:#}"),
            }

            database.close().unwrap();
            return;
        }

        let outputs = concurrent_writers(
            "redis_tests::independent_writers_reject_symbol_aliases",
            &key,
            false,
        );
        let trader_key = database.database.trader_key.clone();
        let mut con = database.database.con.clone();
        let currencies =
            DatabaseQueries::load_currencies(&con, &trader_key, SerializationEncoding::Json)
                .await
                .unwrap();
        let keys: Vec<String> = con.keys(format!("{trader_key}:*")).await.unwrap();
        let _: () = con.del(keys).await.unwrap();
        database.close().unwrap();
        assert_writer_results(outputs);
        assert_eq!(currencies.len(), 1);

        let actual = *currencies.values().next().unwrap();

        let suffix = if actual.code.ends_with("17@ETHEREUM") {
            "17"
        } else {
            "18"
        };

        assert_token(actual, &renamed_token(823_136, suffix));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_instrument_updates_remain_unmarked() {
        let key = format!("DEFI-{}", UUID4::new());
        let database = adapter(&key, SerializationEncoding::MsgPack).await;
        let mut con = database.database.con.clone();
        let trader_key = database.database.trader_key.clone();
        let old = legacy_currency();
        database.add_currency(&old).unwrap();

        let bare =
            DatabaseQueries::serialize_payload(SerializationEncoding::MsgPack, &old).unwrap();
        let _: () = con
            .set(format!("{trader_key}:currencies:{}", old.code), bare)
            .await
            .unwrap();

        let updated = Currency::new(
            old.code.as_str(),
            7,
            0,
            "Updated legacy",
            CurrencyType::Crypto,
        );
        database.add_currency(&updated).unwrap();
        let legacy = DatabaseQueries::load_currency(
            &con,
            &trader_key,
            &old.code,
            SerializationEncoding::MsgPack,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(legacy.precision, 7);
        assert_eq!(legacy.name.as_str(), "Updated legacy");
        assert_eq!(currency::lookup(legacy.code), None);
        let mut cache = Cache::new(None, Some(Box::new(database)));
        let original = legacy_instrument();
        cache.add_instrument(original.clone()).unwrap();
        cache.purge_instrument(original.id());
        let replacement = reversed_instrument(&original);
        cache.add_instrument(replacement.clone()).unwrap();
        let restored = DatabaseQueries::load_instrument(
            &con,
            &trader_key,
            &original.id(),
            SerializationEncoding::MsgPack,
        )
        .await
        .unwrap()
        .unwrap();
        let bytes: Vec<u8> = con
            .get(format!("{trader_key}:instruments:{}", original.id()))
            .await
            .unwrap();
        let value: serde_json::Value =
            DatabaseQueries::deserialize_payload(SerializationEncoding::MsgPack, &bytes).unwrap();
        cache.dispose();
        let keys: Vec<String> = con.keys(format!("{trader_key}:*")).await.unwrap();
        let _: () = con.del(keys).await.unwrap();
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(replacement).unwrap()
        );
        assert_eq!(value.get("defi_currency_codes"), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fresh_process() {
        let child = std::env::var(READER_CASE).ok();
        let key = child
            .clone()
            .unwrap_or_else(|| format!("DEFI-{}", UUID4::new()));
        let mut database = adapter(&key, SerializationEncoding::Json).await;
        let expected = instrument(823_101, false);

        if child.is_some() {
            for token in definitions(823_101) {
                assert!(Currency::try_from_str(&TokenCurrency::from(&token).code()).is_none());
            }

            let InstrumentAny::CurrencyPair(mut untagged) = expected.clone() else {
                unreachable!()
            };

            untagged.id = InstrumentId::from("UNTAGGED.DEFI");
            let error = database.add_instrument(&untagged.into_any()).unwrap_err();
            assert!(
                error.to_string().contains("persisted currency definitions"),
                "{error:#}"
            );
            let restored = database
                .load_instrument(&expected.id())
                .await
                .unwrap()
                .unwrap();
            assert_instrument(&restored, &expected, 823_101);
            let all = database.load_all().await.unwrap();
            assert_eq!(all.instruments.len(), 2);
            assert_eq!(all.currencies.len(), 6);
            assert_eq!(
                serde_json::to_value(&all.instruments[&InstrumentId::from("ETHUSDT.BINANCE")])
                    .unwrap(),
                serde_json::to_value(currency_pair_ethusdt().into_any()).unwrap()
            );
            assert_legacy_definition();
            assert_instrument(&all.instruments[&expected.id()], &expected, 823_101);
            println!("fresh process restored token definitions");
            database.close().unwrap();
            return;
        }

        database.add_currency(&legacy_currency()).unwrap();
        for currency in [Currency::ETH(), Currency::USDT()] {
            database.add_currency(&currency).unwrap();
        }

        database
            .add_instrument(&currency_pair_ethusdt().into_any())
            .unwrap();
        let bare = DatabaseQueries::serialize_payload(SerializationEncoding::Json, &"USD").unwrap();
        database
            .database
            .insert(
                "currencies:USD".into(),
                Some(vec![bytes::Bytes::from(bare)]),
            )
            .unwrap();
        let pair = instrument(823_101, true);
        database.add_instrument(&pair).unwrap();
        reader(
            "redis_tests::fresh_process",
            &key,
            "fresh process restored token definitions",
        );
        let keys = database
            .database
            .con
            .clone()
            .keys::<_, Vec<String>>(format!("{}:*", database.database.trader_key))
            .await
            .unwrap();
        let _: () = database.database.con.clone().del(keys).await.unwrap();
        database.close().unwrap();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn currency_roles_are_immutable(#[case] downgrade: bool) {
        let key = format!("DEFI-{}", UUID4::new());
        let adapter = adapter(&key, SerializationEncoding::Json).await;
        let pair = instrument(823_124, true);
        adapter.add_instrument(&pair).unwrap();
        if downgrade {
            let error = adapter.add_instrument(&legacy_instrument()).unwrap_err();
            assert!(
                error.to_string().contains("currency identities"),
                "{error:#}"
            );
        }

        let trader_key = adapter.database.trader_key.clone();
        let mut con = adapter.database.con.clone();
        let mut cache = Cache::new(None, Some(Box::new(adapter)));

        let replacement = if downgrade {
            legacy_instrument()
        } else {
            reversed_instrument(&pair)
        };

        let error = cache.add_instrument(replacement).unwrap_err();
        let restored = DatabaseQueries::load_instrument(
            &con,
            &trader_key,
            &pair.id(),
            SerializationEncoding::Json,
        )
        .await
        .unwrap()
        .unwrap();
        cache.dispose();
        let keys: Vec<String> = con.keys(format!("{trader_key}:*")).await.unwrap();
        let _: () = con.del(keys).await.unwrap();

        assert!(
            error.to_string().contains("currency identities"),
            "{error:#}"
        );
        assert_instrument(&restored, &pair, 823_124);
        assert_eq!(cache.instrument(&pair.id()), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn independent_writers_reject_precision_conflict() {
        let child = std::env::var(READER_CASE).ok();
        let key = child
            .clone()
            .unwrap_or_else(|| format!("DEFI-{}", UUID4::new()));
        let mut adapter = adapter(&key, SerializationEncoding::Json).await;

        if child.is_some() {
            let reversed = std::env::var("NAUTILUS_DEFI_WRITER_CASE").unwrap() == "18";
            match adapter.add_instrument(&conflict_instrument(823_126, reversed)) {
                Ok(()) => println!("WRITER_COMMITTED"),
                Err(e) => assert!(
                    e.to_string()
                        .contains("Conflicting DeFi currency definition"),
                    "{e:#}"
                ),
            }

            adapter.close().unwrap();
            return;
        }

        let outputs = concurrent_writers(
            "redis_tests::independent_writers_reject_precision_conflict",
            &key,
            false,
        );
        let restored = adapter
            .load_instrument(&InstrumentId::from("ABCABC.DEFI"))
            .await
            .unwrap()
            .unwrap();
        let keys: Vec<String> = adapter
            .database
            .con
            .clone()
            .keys(format!("{}:*", adapter.database.trader_key))
            .await
            .unwrap();
        let _: () = adapter.database.con.clone().del(keys).await.unwrap();
        adapter.close().unwrap();

        assert_writer_results(outputs);
        let precision = restored.base_currency().unwrap().precision;
        assert!([17, 18].contains(&precision));
        assert_instrument(
            &restored,
            &conflict_instrument(823_126, precision == 18),
            823_126,
        );
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_metadata_fails(#[case] encoding: SerializationEncoding) {
        let key = format!("DEFI-{}", UUID4::new());
        let mut database = adapter(&key, encoding).await;
        let pair = instrument(823_102, true);
        database.add_instrument(&pair).unwrap();
        let currency = pair.base_currency().unwrap();
        let currency_key = format!(
            "{}:currencies:{}",
            database.database.trader_key, currency.code
        );
        let mut con = database.database.con.clone();
        let raw: Vec<u8> = con.get(&currency_key).await.unwrap();
        let mut value: serde_json::Value =
            DatabaseQueries::deserialize_payload(encoding, &raw).unwrap();
        value.as_object_mut().unwrap().remove("defi");
        let altered = DatabaseQueries::serialize_payload(encoding, &value).unwrap();
        let _: () = con.set(&currency_key, altered).await.unwrap();
        assert!(database.load_instrument(&pair.id()).await.is_err());
        assert!(database.load_instruments().await.is_err());
        assert!(database.load_all().await.is_err());
        let _: () = con.set(&currency_key, raw).await.unwrap();
        let _: () = con.del(&currency_key).await.unwrap();
        assert!(database.load_instrument(&pair.id()).await.is_err());
        assert!(database.load_instruments().await.is_err());
        let keys: Vec<String> = con
            .keys(format!("{}:*", database.database.trader_key))
            .await
            .unwrap();
        let _: () = con.del(keys).await.unwrap();
        database.close().unwrap();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn precision_conflict_in_both_orders(#[case] reversed: bool) {
        let key = format!("DEFI-{}", UUID4::new());
        let mut database = adapter(&key, SerializationEncoding::Json).await;
        let pair = conflict_instrument(if reversed { 823_107 } else { 823_106 }, reversed);
        database.add_instrument(&pair).unwrap();
        let code = pair.base_currency().unwrap().code;
        let record_key = format!("{}:currencies:{code}", database.database.trader_key);
        let mut con = database.database.con.clone();
        let raw: Vec<u8> = con.get(&record_key).await.unwrap();
        let mut value: serde_json::Value =
            DatabaseQueries::deserialize_payload(SerializationEncoding::Json, &raw).unwrap();
        value["precision"] = serde_json::Value::from(if reversed { 17 } else { 18 });
        let payload =
            DatabaseQueries::serialize_payload(SerializationEncoding::Json, &value).unwrap();
        let _: () = con.set(record_key, payload).await.unwrap();
        let error = database.load_instruments().await.unwrap_err();
        let keys: Vec<String> = con
            .keys(format!("{}:*", database.database.trader_key))
            .await
            .unwrap();
        let _: () = con.del(keys).await.unwrap();
        database.close().unwrap();
        assert!(
            error
                .to_string()
                .contains(&format!("Conflicting DeFi currency definition for {code}")),
            "{error:#}"
        );
    }

    #[rstest]
    #[case("clear")]
    #[case("history")]
    #[case("archive")]
    #[case("unrelated_archive")]
    #[case("orders")]
    #[case("order_snapshot")]
    #[case("intents")]
    #[case("unavailable")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn historical_replacement(#[case] mode: &str) {
        let key = format!("DEFI-{}", UUID4::new());
        let adapter = adapter(&key, SerializationEncoding::Json).await;
        let legacy = legacy_instrument();
        adapter.add_instrument(&legacy).unwrap();
        if mode == "history" {
            let snapshot = PositionSnapshot::from_replay_state(&closed_position(&legacy), None);
            adapter.add_position_snapshot(&snapshot).unwrap();
        }

        if mode == "archive" || mode == "unrelated_archive" {
            adapter
                .add(
                    format!("{}archived/0", CacheSnapshotRef::POSITION_PREFIX),
                    position_archive(mode, &legacy).into(),
                )
                .unwrap();
        }

        if mode == "orders" || mode == "order_snapshot" {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(legacy.id())
                .quantity(Quantity::from("1.00000"))
                .build();

            if mode == "orders" {
                adapter.add_order(&order, None).unwrap();
            } else {
                adapter
                    .add_order_snapshot(&OrderSnapshot::from(order))
                    .unwrap();
            }
        }

        let trader_key = adapter.database.trader_key.clone();
        let mut con = adapter.database.con.clone();
        let mut cache = Cache::new(None, Some(Box::new(adapter)));

        if mode != "unavailable" {
            let active = mode == "intents";
            cache.register_defi_execution_check(ClientId::from("DEFI"), 823_120, move |_| {
                anyhow::ensure!(!active, "active swap intents");
                Ok(())
            });
        }

        let pair = instrument(823_120, true);

        let result = cache.add_instrument(pair.clone());
        let restored = DatabaseQueries::load_instrument(
            &con,
            &trader_key,
            &pair.id(),
            SerializationEncoding::Json,
        )
        .await
        .unwrap()
        .unwrap();
        let cached = cache.instrument(&pair.id()).cloned();
        cache.dispose();
        let keys: Vec<String> = con.keys(format!("{trader_key}:*")).await.unwrap();
        let _: () = con.del(keys).await.unwrap();

        if mode == "clear" || mode == "unrelated_archive" {
            result.unwrap();
            assert_instrument(&restored, &pair, 823_120);
            assert_instrument(&cached.unwrap(), &pair, 823_120);
        } else {
            let error = result.unwrap_err().to_string();

            let expected = replacement_failure(mode);

            assert!(error.contains(expected), "{error}");
            assert_eq!(
                serde_json::to_value(restored).unwrap(),
                serde_json::to_value(legacy).unwrap()
            );
            assert_eq!(cached, None);
        }
    }
}

#[cfg(feature = "postgres")]
mod postgres_tests {
    use nautilus_infrastructure::sql::{
        cache::PostgresCacheDatabase, pg::get_postgres_connect_options, queries::DatabaseQueries,
    };
    use rstest::rstest;
    use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions};

    use super::*;

    #[rstest]
    #[case(true)]
    #[case(false)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn startup_requires_defi_schema(#[case] missing_table: bool) {
        let child = std::env::var(READER_CASE).ok();
        let schema = child
            .clone()
            .unwrap_or_else(|| format!("defi_currency_{}", UUID4::new()).replace('-', "_"));
        let pool = pool(&schema, child.is_none()).await;
        if child.is_some() {
            let error =
                PostgresCacheDatabase::connect(Default::default(), TraderId::from("DEFI-001"))
                    .await
                    .unwrap_err();

            let missing = if missing_table {
                "defi_currency.currency_id"
            } else {
                "instrument.defi_currency_codes"
            };

            assert!(error.to_string().contains(missing), "{error}");
            assert!(
                error.to_string().contains("nautilus database init"),
                "{error}"
            );
            println!("UNMIGRATED_SCHEMA_REJECTED");
        } else {
            let statement = if missing_table {
                "DROP TABLE defi_currency"
            } else {
                "ALTER TABLE instrument DROP COLUMN defi_currency_codes"
            };

            sqlx::query(AssertSqlSafe(statement))
                .execute(&pool)
                .await
                .unwrap();

            let test = if missing_table {
                "postgres_tests::startup_requires_defi_schema::case_1"
            } else {
                "postgres_tests::startup_requires_defi_schema::case_2"
            };

            reader(test, &schema, "UNMIGRATED_SCHEMA_REJECTED");

            for _ in 0..2 {
                sqlx::raw_sql(include_str!("../../../schema/sql/tables.sql"))
                    .execute(&pool)
                    .await
                    .unwrap();
            }

            sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                .execute(&pool)
                .await
                .unwrap();
        }

        pool.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cold_cache_rejects_legacy_downgrade() {
        let child = std::env::var(READER_CASE).ok();
        let schema = child
            .clone()
            .unwrap_or_else(|| format!("defi_currency_{}", UUID4::new()).replace('-', "_"));
        let pool = pool(&schema, child.is_none()).await;
        if child.is_some() {
            let adapter =
                PostgresCacheDatabase::connect(Default::default(), TraderId::from("DEFI-001"))
                    .await
                    .unwrap();
            let mut cache = Cache::new(None, Some(Box::new(adapter)));
            let error = cache.add_instrument(legacy_instrument()).unwrap_err();
            assert_eq!(cache.instrument(&InstrumentId::from("ABCABC.DEFI")), None);
            cache.dispose();
            assert!(
                error.to_string().contains("currency identities"),
                "{error:#}"
            );
            println!("DOWNGRADE_REJECTED");
        } else {
            let pair = instrument(823_137, true);
            DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(pair.clone()))
                .await
                .unwrap();
            reader(
                "postgres_tests::cold_cache_rejects_legacy_downgrade",
                &schema,
                "DOWNGRADE_REJECTED",
            );
            let restored = DatabaseQueries::load_instrument(&pool, &pair.id())
                .await
                .unwrap()
                .unwrap();
            sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                .execute(&pool)
                .await
                .unwrap();
            assert_instrument(&restored, &pair, 823_137);
        }

        pool.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn renamed_contract_is_rejected() {
        let child = std::env::var(READER_CASE).ok();
        let schema = child
            .clone()
            .unwrap_or_else(|| format!("defi_currency_{}", UUID4::new()).replace('-', "_"));
        let pool = pool(&schema, child.is_none()).await;
        let original = definitions(823_138)[0].clone();
        if child.is_some() {
            let metadata = TokenCurrency::from(&original);

            let untagged = Currency::new(
                metadata.code().as_str(),
                6,
                0,
                "Stale legacy definition",
                CurrencyType::Crypto,
            );
            let legacy_error = DatabaseQueries::add_currency(&pool, untagged)
                .await
                .unwrap_err();
            assert!(
                legacy_error.to_string().contains("Missing token metadata"),
                "{legacy_error:#}"
            );
            let renamed = renamed_token(823_138, "").currency().unwrap();
            let error = DatabaseQueries::add_currency(&pool, renamed)
                .await
                .unwrap_err();
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM currency WHERE id = $1")
                .bind(renamed.code.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
            assert!(
                error.to_string().contains(&format!(
                    "already persisted as {}",
                    TokenCurrency::from(&original).code()
                )),
                "{error:#}"
            );
            assert_eq!(count, 0);
            println!("RENAMED_WRITE_REJECTED");
        } else {
            DatabaseQueries::add_currency(&pool, original.currency().unwrap())
                .await
                .unwrap();
            reader(
                "postgres_tests::renamed_contract_is_rejected",
                &schema,
                "RENAMED_WRITE_REJECTED",
            );
            let currencies = DatabaseQueries::load_currencies(&pool).await.unwrap();
            sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                .execute(&pool)
                .await
                .unwrap();
            assert_eq!(currencies.len(), 1);
            assert_token(currencies[0], &original);
        }

        pool.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn independent_writers_reject_symbol_aliases() {
        let child = std::env::var(READER_CASE).ok();
        let schema = child
            .clone()
            .unwrap_or_else(|| format!("defi_currency_{}", UUID4::new()).replace('-', "_"));
        let pool = pool(&schema, child.is_none()).await;
        if child.is_some() {
            let suffix = std::env::var("NAUTILUS_DEFI_WRITER_CASE").unwrap();
            let token = renamed_token(823_139, &suffix);
            match DatabaseQueries::add_currency(&pool, token.currency().unwrap()).await {
                Ok(()) => println!("WRITER_COMMITTED"),
                Err(e) => assert!(e.to_string().contains("already persisted as"), "{e:#}"),
            }
        } else {
            let outputs = concurrent_writers(
                "postgres_tests::independent_writers_reject_symbol_aliases",
                &schema,
                true,
            );
            let currencies = DatabaseQueries::load_currencies(&pool).await.unwrap();
            sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                .execute(&pool)
                .await
                .unwrap();
            assert_writer_results(outputs);
            assert_eq!(currencies.len(), 1);

            let suffix = if currencies[0].code.ends_with("17@ETHEREUM") {
                "17"
            } else {
                "18"
            };

            assert_token(currencies[0], &renamed_token(823_139, suffix));
        }

        pool.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_instrument_updates_remain_unmarked() {
        let schema = format!("defi_currency_{}", UUID4::new()).replace('-', "_");
        let pool = pool(&schema, true).await;
        let original = legacy_instrument();
        for c in [original.base_currency().unwrap(), original.quote_currency()] {
            DatabaseQueries::add_currency(&pool, c).await.unwrap();
        }

        DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(original.clone()))
            .await
            .unwrap();
        let replacement = reversed_instrument(&original);
        DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(replacement.clone()))
            .await
            .unwrap();
        let restored = DatabaseQueries::load_instrument(&pool, &original.id())
            .await
            .unwrap()
            .unwrap();
        let marker: Option<Vec<String>> =
            sqlx::query_scalar("SELECT defi_currency_codes FROM instrument WHERE id = $1")
                .bind(original.id().to_string())
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(replacement).unwrap()
        );
        assert_eq!(marker, None);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn currency_roles_are_immutable(#[case] downgrade: bool) {
        let schema = format!("defi_currency_{}", UUID4::new()).replace('-', "_");
        let pool = pool(&schema, true).await;
        let pair = instrument(823_125, true);
        DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(pair.clone()))
            .await
            .unwrap();

        let error = DatabaseQueries::add_instrument(
            &pool,
            "CURRENCY_PAIR",
            Box::new(if downgrade {
                legacy_instrument()
            } else {
                reversed_instrument(&pair)
            }),
        )
        .await
        .unwrap_err();

        let restored = DatabaseQueries::load_instrument(&pool, &pair.id())
            .await
            .unwrap()
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        assert!(
            error.to_string().contains("currency identities"),
            "{error:#}"
        );
        assert_instrument(&restored, &pair, 823_125);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn independent_writers_reject_precision_conflict() {
        let child = std::env::var(READER_CASE).ok();
        let schema = child
            .clone()
            .unwrap_or_else(|| format!("defi_currency_{}", UUID4::new()).replace('-', "_"));
        let pool = pool(&schema, child.is_none()).await;
        if child.is_some() {
            let reversed = std::env::var("NAUTILUS_DEFI_WRITER_CASE").unwrap() == "18";

            match DatabaseQueries::add_instrument(
                &pool,
                "CURRENCY_PAIR",
                Box::new(conflict_instrument(823_127, reversed)),
            )
            .await
            {
                Ok(()) => println!("WRITER_COMMITTED"),
                Err(e) => assert!(
                    e.to_string()
                        .contains("Conflicting DeFi currency definition"),
                    "{e:#}"
                ),
            }

            pool.close().await;
            return;
        }

        let outputs = concurrent_writers(
            "postgres_tests::independent_writers_reject_precision_conflict",
            &schema,
            true,
        );
        let restored = DatabaseQueries::load_instrument(&pool, &InstrumentId::from("ABCABC.DEFI"))
            .await
            .unwrap()
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        assert_writer_results(outputs);
        let precision = restored.base_currency().unwrap().precision;
        assert!([17, 18].contains(&precision));
        assert_instrument(
            &restored,
            &conflict_instrument(823_127, precision == 18),
            823_127,
        );
    }

    async fn pool(schema: &str, create: bool) -> PgPool {
        let options = get_postgres_connect_options(None, None, None, None, None);
        let admin = PgPool::connect_with(options.clone().into()).await.unwrap();

        if create {
            sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
                .execute(&admin)
                .await
                .unwrap();
        }

        admin.close().await;
        let search_path = format!("SET search_path TO {schema}");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |connection, _| {
                let query = search_path.clone();
                Box::pin(async move {
                    sqlx::query(AssertSqlSafe(query))
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options.into())
            .await
            .unwrap();

        if create {
            sqlx::raw_sql(include_str!("../../../schema/sql/types.sql"))
                .execute(&pool)
                .await
                .unwrap();
            let tables = include_str!("../../../schema/sql/tables.sql");
            sqlx::raw_sql(tables).execute(&pool).await.unwrap();
            sqlx::raw_sql(tables).execute(&pool).await.unwrap();
        }

        pool
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fresh_process() {
        let child = std::env::var(READER_CASE).ok();
        let schema = child
            .clone()
            .unwrap_or_else(|| format!("defi_currency_{}", UUID4::new()).replace('-', "_"));
        let pool = pool(&schema, child.is_none()).await;
        let expected = instrument(823_103, false);

        if child.is_some() {
            for token in definitions(823_103) {
                assert!(Currency::try_from_str(&TokenCurrency::from(&token).code()).is_none());
            }

            let InstrumentAny::CurrencyPair(mut untagged) = expected.clone() else {
                unreachable!()
            };

            untagged.id = InstrumentId::from("UNTAGGED.DEFI");
            let error = DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(untagged))
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("persisted currency definitions"),
                "{error:#}"
            );
            let restored = DatabaseQueries::load_instrument(&pool, &expected.id())
                .await
                .unwrap()
                .unwrap();
            assert_instrument(&restored, &expected, 823_103);
            let all = DatabaseQueries::load_instruments(&pool).await.unwrap();
            assert_eq!(all.len(), 2);
            let restored = all.iter().find(|pair| pair.id() == expected.id()).unwrap();
            assert_instrument(restored, &expected, 823_103);
            let currencies = DatabaseQueries::load_currencies(&pool).await.unwrap();
            assert_eq!(currencies.len(), 5);
            nautilus_common::cache::database::register_loaded_currencies(
                &mut currencies
                    .into_iter()
                    .map(|currency| (currency.code, currency))
                    .collect(),
            )
            .unwrap();
            assert_legacy_definition();
            println!("fresh process restored token definitions");
            pool.close().await;
            return;
        }

        for currency in [legacy_currency(), Currency::ETH(), Currency::USDT()] {
            DatabaseQueries::add_currency(&pool, currency)
                .await
                .unwrap();
        }

        DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(currency_pair_ethusdt()))
            .await
            .unwrap();
        let pair = instrument(823_103, true);
        DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(pair))
            .await
            .unwrap();
        reader(
            "postgres_tests::fresh_process",
            &schema,
            "fresh process restored token definitions",
        );
        sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_metadata_fails() {
        let schema = format!("defi_currency_{}", UUID4::new()).replace('-', "_");
        let pool = pool(&schema, true).await;
        let pair = instrument(823_104, true);
        DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(pair.clone()))
            .await
            .unwrap();
        sqlx::query("DELETE FROM defi_currency WHERE currency_id = $1")
            .bind(pair.base_currency().unwrap().code.as_str())
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            DatabaseQueries::load_instrument(&pool, &pair.id())
                .await
                .is_err()
        );
        assert!(DatabaseQueries::load_instruments(&pool).await.is_err());
        sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn precision_conflict_in_both_orders(#[case] reversed: bool) {
        let schema = format!("defi_currency_{}", UUID4::new()).replace('-', "_");
        let pool = pool(&schema, true).await;
        let pair = conflict_instrument(if reversed { 823_109 } else { 823_108 }, reversed);
        DatabaseQueries::add_instrument(&pool, "CURRENCY_PAIR", Box::new(pair.clone()))
            .await
            .unwrap();
        let code = pair.base_currency().unwrap().code;
        sqlx::query("UPDATE currency SET precision = $2 WHERE id = $1")
            .bind(code.as_str())
            .bind(if reversed { 17 } else { 18 })
            .execute(&pool)
            .await
            .unwrap();
        let error = DatabaseQueries::load_instruments(&pool).await.unwrap_err();
        sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert!(
            error
                .to_string()
                .contains(&format!("Conflicting DeFi currency definition for {code}")),
            "{error:#}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn historical_replacement() {
        const MODE: &str = "NAUTILUS_DEFI_MIGRATION_CASE";

        if let Ok(schema) = std::env::var(READER_CASE) {
            let mode = std::env::var(MODE).unwrap();
            let pool = pool(&schema, false).await;
            let adapter =
                PostgresCacheDatabase::connect(Default::default(), TraderId::from("DEFI-001"))
                    .await
                    .unwrap();
            let mut cache = Cache::new(None, Some(Box::new(adapter)));

            if mode != "unavailable" {
                let active = mode == "intents";
                cache.register_defi_execution_check(ClientId::from("DEFI"), 823_121, move |_| {
                    anyhow::ensure!(!active, "active swap intents");
                    Ok(())
                });
            }

            let pair = instrument(823_121, true);
            let result = cache.add_instrument(pair.clone());
            let restored = DatabaseQueries::load_instrument(&pool, &pair.id())
                .await
                .unwrap()
                .unwrap();
            let cached = cache.instrument(&pair.id()).cloned();
            cache.dispose();
            pool.close().await;

            if mode == "clear" || mode == "unrelated_archive" {
                result.unwrap();
                assert_instrument(&restored, &pair, 823_121);
                assert_instrument(&cached.unwrap(), &pair, 823_121);
            } else {
                let error = result.unwrap_err().to_string();

                let expected = replacement_failure(mode.as_ref());

                assert!(error.contains(expected), "{error}");
                assert_eq!(
                    serde_json::to_value(restored).unwrap(),
                    serde_json::to_value(legacy_instrument()).unwrap()
                );
                assert_eq!(cached, None);
            }

            return;
        }

        for mode in [
            "clear",
            "history",
            "archive",
            "unrelated_archive",
            "orders",
            "intents",
            "unavailable",
        ] {
            let schema = format!("defi_currency_{}", UUID4::new()).replace('-', "_");
            let pool = pool(&schema, true).await;
            store_legacy_state(&pool, mode).await;

            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "postgres_tests::historical_replacement",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(READER_CASE, &schema)
                .env(MODE, mode)
                .env("NAUTILUS_WORKER_THREADS", "2")
                .env("PGOPTIONS", format!("-c search_path={schema}"))
                .output()
                .unwrap();
            sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
            assert!(
                output.status.success(),
                "Case {mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    async fn store_legacy_state(pool: &PgPool, mode: &str) {
        let legacy = legacy_instrument();
        for currency in [legacy.base_currency().unwrap(), legacy.quote_currency()] {
            DatabaseQueries::add_currency(pool, currency).await.unwrap();
        }

        DatabaseQueries::add_instrument(pool, "CURRENCY_PAIR", Box::new(legacy.clone()))
            .await
            .unwrap();

        if mode == "history" {
            let position = closed_position(&legacy);
            sqlx::query("INSERT INTO trader (id) VALUES ($1)")
                .bind(position.trader_id.to_string())
                .execute(pool)
                .await
                .unwrap();
            let snapshot = PositionSnapshot::from_replay_state(&position, None);
            DatabaseQueries::add_position_snapshot(pool, snapshot)
                .await
                .unwrap();
        }

        if mode == "archive" || mode == "unrelated_archive" {
            DatabaseQueries::add(
                pool,
                format!("{}archived/0", CacheSnapshotRef::POSITION_PREFIX),
                position_archive(mode, &legacy),
            )
            .await
            .unwrap();
        }

        if mode == "orders" {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(legacy.id())
                .quantity(Quantity::from("1.00000"))
                .build();
            sqlx::query("INSERT INTO trader (id) VALUES ($1)")
                .bind(order.trader_id().to_string())
                .execute(pool)
                .await
                .unwrap();
            DatabaseQueries::add_order(pool, order.init_event().clone(), None)
                .await
                .unwrap();
        }
    }
}
