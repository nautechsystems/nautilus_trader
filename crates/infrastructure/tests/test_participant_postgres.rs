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

#[cfg(test)]
#[cfg(feature = "postgres")]
#[cfg(target_os = "linux")]
mod tests {
    use std::str::FromStr;

    use nautilus_core::UnixNanos;
    use nautilus_infrastructure::sql::{
        pg::{connect_pg, get_postgres_connect_options},
        queries::DatabaseQueries,
    };
    use nautilus_model::{
        data::{
            Participant, ParticipantKind, ParticipantProfile, ParticipantTransaction,
            TransactionMethod,
        },
        enums::CurrencyType,
        identifiers::{InstrumentId, ParticipantId, Venue},
        types::{AccountBalance, Currency, MarginBalance, Money, Price},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use sqlx::{PgPool, Row};

    async fn setup_pool() -> PgPool {
        let options = get_postgres_connect_options(None, None, None, None, None);
        connect_pg(options.into())
            .await
            .expect("Failed to connect to test database")
    }

    fn test_participant(id: &str) -> Participant {
        Participant::new(
            ParticipantId::new(id),
            Venue::new("HYPERLIQUID"),
            ParticipantKind::Wallet,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
        )
    }

    fn test_balance() -> AccountBalance {
        let currency = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
        AccountBalance::new(
            Money::new(15332.500548, currency),
            Money::new(15271.628544, currency),
            Money::new(60.872004, currency),
        )
    }

    fn test_margin() -> MarginBalance {
        let currency = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
        MarginBalance::new_checked(
            Money::new(1000.50, currency),
            Money::new(800.25, currency),
            None,
        )
        .unwrap()
    }

    fn test_transaction() -> ParticipantTransaction {
        let currency = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
        ParticipantTransaction::new(
            "0xabc123".into(),
            TransactionMethod::OpenLong,
            UnixNanos::from(5_000_000_000u64),
            Decimal::from_str("100.5").unwrap(),
            InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"),
            Price::from("50000.00"),
            Money::new(5025000.00, currency),
        )
    }

    fn test_profile(participant_id: &str) -> ParticipantProfile {
        ParticipantProfile::new(
            ParticipantId::new(participant_id),
            Some(vec![test_balance()]),
            Some(vec![test_margin()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![test_transaction()]),
            UnixNanos::from(10_000_000_000u64),
        )
    }

    async fn cleanup(pool: &PgPool, participant_id: &str) {
        // Clean up in reverse FK order
        let pk_row =
            sqlx::query("SELECT participant_pk FROM participant WHERE participant_id = $1")
                .bind(participant_id)
                .fetch_optional(pool)
                .await
                .unwrap();

        if let Some(row) = pk_row {
            let pk: i64 = row.get("participant_pk");
            sqlx::query("DELETE FROM participant_profile_transaction WHERE participant_pk = $1")
                .bind(pk)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM participant_profile_open_order WHERE participant_pk = $1")
                .bind(pk)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM participant_profile_position WHERE participant_pk = $1")
                .bind(pk)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM participant_profile_margin WHERE participant_pk = $1")
                .bind(pk)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM participant_profile_balance WHERE participant_pk = $1")
                .bind(pk)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM participant_profile WHERE participant_pk = $1")
                .bind(pk)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM participant WHERE participant_id = $1")
            .bind(participant_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn test_upsert_participants_inserts_and_updates() {
        let pool = setup_pool().await;
        let pid = "0xTEST_UPSERT_PARTICIPANT";
        cleanup(&pool, pid).await;

        let participant = test_participant(pid);
        DatabaseQueries::upsert_participants(&pool, &[participant])
            .await
            .unwrap();

        let loaded = DatabaseQueries::load_participant(
            &pool,
            &Venue::new("HYPERLIQUID"),
            &ParticipantId::new(pid),
        )
        .await
        .unwrap()
        .expect("participant should exist");

        assert_eq!(loaded.id, ParticipantId::new(pid));
        assert_eq!(loaded.first_seen_at, UnixNanos::from(1_000_000_000u64));
        assert_eq!(loaded.last_seen_at, UnixNanos::from(2_000_000_000u64));

        // Update with earlier first_seen and later last_seen
        let updated = Participant::new(
            ParticipantId::new(pid),
            Venue::new("HYPERLIQUID"),
            ParticipantKind::Wallet,
            UnixNanos::from(500_000_000u64),
            UnixNanos::from(3_000_000_000u64),
            UnixNanos::from(4_000_000_000u64),
        );
        DatabaseQueries::upsert_participants(&pool, &[updated])
            .await
            .unwrap();

        let reloaded = DatabaseQueries::load_participant(
            &pool,
            &Venue::new("HYPERLIQUID"),
            &ParticipantId::new(pid),
        )
        .await
        .unwrap()
        .expect("participant should exist after update");

        assert_eq!(reloaded.first_seen_at, UnixNanos::from(500_000_000u64));
        assert_eq!(reloaded.last_seen_at, UnixNanos::from(3_000_000_000u64));

        cleanup(&pool, pid).await;
    }

    #[rstest]
    #[tokio::test]
    async fn test_upsert_profiles_balance_stores_decimal_without_currency_suffix() {
        let pool = setup_pool().await;
        let pid = "0xTEST_BALANCE_DECIMAL";
        cleanup(&pool, pid).await;

        let participant = test_participant(pid);
        DatabaseQueries::upsert_participants(&pool, &[participant])
            .await
            .unwrap();

        let profile = test_profile(pid);
        DatabaseQueries::upsert_participant_profiles(&pool, &[profile])
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT b.total, b.locked, b.free
             FROM participant_profile_balance b
             JOIN participant p ON p.participant_pk = b.participant_pk
             WHERE p.participant_id = $1",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

        let total: String = row.get("total");
        let locked: String = row.get("locked");
        let free: String = row.get("free");

        // Must be plain decimals without currency suffix
        assert!(
            !total.contains("USDC"),
            "total should not contain currency: {total}"
        );
        assert!(
            !locked.contains("USDC"),
            "locked should not contain currency: {locked}"
        );
        assert!(
            !free.contains("USDC"),
            "free should not contain currency: {free}"
        );

        assert_eq!(total, "15332.500548");
        assert_eq!(locked, "15271.628544");
        assert_eq!(free, "60.872004");

        cleanup(&pool, pid).await;
    }

    #[rstest]
    #[tokio::test]
    async fn test_upsert_profiles_margin_stores_decimal_without_currency_suffix() {
        let pool = setup_pool().await;
        let pid = "0xTEST_MARGIN_DECIMAL";
        cleanup(&pool, pid).await;

        let participant = test_participant(pid);
        DatabaseQueries::upsert_participants(&pool, &[participant])
            .await
            .unwrap();

        let profile = test_profile(pid);
        DatabaseQueries::upsert_participant_profiles(&pool, &[profile])
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT m.initial, m.maintenance
             FROM participant_profile_margin m
             JOIN participant p ON p.participant_pk = m.participant_pk
             WHERE p.participant_id = $1",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

        let initial: String = row.get("initial");
        let maintenance: String = row.get("maintenance");

        assert!(
            !initial.contains("USDC"),
            "initial should not contain currency: {initial}"
        );
        assert!(
            !maintenance.contains("USDC"),
            "maintenance should not contain currency: {maintenance}"
        );

        assert_eq!(initial, "1000.500000");
        assert_eq!(maintenance, "800.250000");

        cleanup(&pool, pid).await;
    }

    #[rstest]
    #[tokio::test]
    async fn test_upsert_profiles_transactions_persisted() {
        let pool = setup_pool().await;
        let pid = "0xTEST_TXN_PERSIST";
        cleanup(&pool, pid).await;

        let participant = test_participant(pid);
        DatabaseQueries::upsert_participants(&pool, &[participant])
            .await
            .unwrap();

        let profile = test_profile(pid);
        DatabaseQueries::upsert_participant_profiles(&pool, &[profile])
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT t.hash, t.method, t.amount, t.instrument_id, t.price,
                    t.value_amount, t.value_currency
             FROM participant_profile_transaction t
             JOIN participant p ON p.participant_pk = t.participant_pk
             WHERE p.participant_id = $1",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("hash"), "0xabc123");
        assert_eq!(row.get::<String, _>("method"), "OPEN_LONG");
        assert_eq!(row.get::<String, _>("amount"), "100.5");
        assert_eq!(
            row.get::<String, _>("instrument_id"),
            "BTC-USD-PERP.HYPERLIQUID"
        );
        assert_eq!(row.get::<String, _>("price"), "50000.00");
        assert_eq!(row.get::<String, _>("value_amount"), "5025000.000000");
        assert_eq!(row.get::<String, _>("value_currency"), "USDC");

        // Value should not have currency suffix
        let value_amount: String = row.get("value_amount");
        assert!(
            !value_amount.contains("USDC"),
            "value_amount should be plain decimal: {value_amount}"
        );

        cleanup(&pool, pid).await;
    }

    #[rstest]
    #[tokio::test]
    async fn test_upsert_profiles_transactions_dedup_by_hash() {
        let pool = setup_pool().await;
        let pid = "0xTEST_TXN_DEDUP";
        cleanup(&pool, pid).await;

        let participant = test_participant(pid);
        DatabaseQueries::upsert_participants(&pool, &[participant])
            .await
            .unwrap();

        let profile = test_profile(pid);

        // Insert same profile twice — transaction has same hash
        DatabaseQueries::upsert_participant_profiles(&pool, std::slice::from_ref(&profile))
            .await
            .unwrap();
        DatabaseQueries::upsert_participant_profiles(&pool, std::slice::from_ref(&profile))
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM participant_profile_transaction t
             JOIN participant p ON p.participant_pk = t.participant_pk
             WHERE p.participant_id = $1",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(count, 1, "duplicate hash should be deduplicated");

        cleanup(&pool, pid).await;
    }

    #[rstest]
    #[tokio::test]
    async fn test_upsert_profiles_balances_replaced_on_refresh() {
        let pool = setup_pool().await;
        let pid = "0xTEST_BALANCE_REPLACE";
        cleanup(&pool, pid).await;

        let participant = test_participant(pid);
        DatabaseQueries::upsert_participants(&pool, &[participant])
            .await
            .unwrap();

        // First profile with one balance
        let profile1 = test_profile(pid);
        DatabaseQueries::upsert_participant_profiles(&pool, &[profile1])
            .await
            .unwrap();

        let count1: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM participant_profile_balance b
             JOIN participant p ON p.participant_pk = b.participant_pk
             WHERE p.participant_id = $1",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count1, 1);

        // Second profile with two balances — should replace, not append
        let currency_usdc = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
        let currency_btc = Currency::new("BTC", 8, 0, "BTC", CurrencyType::Crypto);
        let profile2 = ParticipantProfile::new(
            ParticipantId::new(pid),
            Some(vec![
                AccountBalance::new(
                    Money::new(100.0, currency_usdc),
                    Money::new(0.0, currency_usdc),
                    Money::new(100.0, currency_usdc),
                ),
                AccountBalance::new(
                    Money::new(0.5, currency_btc),
                    Money::new(0.0, currency_btc),
                    Money::new(0.5, currency_btc),
                ),
            ]),
            None,
            None,
            None,
            None,
            UnixNanos::from(20_000_000_000u64),
        );
        DatabaseQueries::upsert_participant_profiles(&pool, &[profile2])
            .await
            .unwrap();

        let count2: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM participant_profile_balance b
             JOIN participant p ON p.participant_pk = b.participant_pk
             WHERE p.participant_id = $1",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count2, 2, "balances should be replaced, not appended");

        cleanup(&pool, pid).await;
    }

    #[rstest]
    #[tokio::test]
    async fn test_upsert_profiles_transactions_append_not_replace() {
        let pool = setup_pool().await;
        let pid = "0xTEST_TXN_APPEND";
        cleanup(&pool, pid).await;

        let participant = test_participant(pid);
        DatabaseQueries::upsert_participants(&pool, &[participant])
            .await
            .unwrap();

        let currency = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);

        // First profile with one transaction
        let profile1 = ParticipantProfile::new(
            ParticipantId::new(pid),
            None,
            None,
            None,
            None,
            Some(vec![ParticipantTransaction::new(
                "0xhash1".into(),
                TransactionMethod::OpenLong,
                UnixNanos::from(1_000_000_000u64),
                Decimal::from_str("10.0").unwrap(),
                InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"),
                Price::from("50000.00"),
                Money::new(500000.0, currency),
            )]),
            UnixNanos::from(10_000_000_000u64),
        );
        DatabaseQueries::upsert_participant_profiles(&pool, &[profile1])
            .await
            .unwrap();

        // Second profile with a different transaction
        let profile2 = ParticipantProfile::new(
            ParticipantId::new(pid),
            None,
            None,
            None,
            None,
            Some(vec![ParticipantTransaction::new(
                "0xhash2".into(),
                TransactionMethod::OpenLong,
                UnixNanos::from(2_000_000_000u64),
                Decimal::from_str("20.0").unwrap(),
                InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"),
                Price::from("3000.00"),
                Money::new(60000.0, currency),
            )]),
            UnixNanos::from(20_000_000_000u64),
        );
        DatabaseQueries::upsert_participant_profiles(&pool, &[profile2])
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM participant_profile_transaction t
             JOIN participant p ON p.participant_pk = t.participant_pk
             WHERE p.participant_id = $1",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(count, 2, "transactions should append, not replace");

        cleanup(&pool, pid).await;
    }
}
