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

use std::{collections::HashMap, fmt::Display};

use nautilus_core::{Params, UUID4, UnixNanos};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

use crate::{
    enums::AccountType,
    identifiers::{AccountId, InstrumentId},
    types::{AccountBalance, Currency, MarginBalance, balance::WalletAccountBalances},
};

/// Represents an event which includes information on the state of the account.
///
/// The optional `info` bag carries venue-specific account data that does not map
/// to the typed `balances` and `margins` fields, such as wallet balance,
/// available balance, or an account summary, so consumers can read the venue
/// context that accompanied a given snapshot.
#[repr(C)]
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct AccountState {
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The type of the account (e.g., margin, spot, etc.).
    pub account_type: AccountType,
    /// The base currency for the account, if applicable.
    pub base_currency: Option<Currency>,
    /// The balances in the account.
    pub balances: Vec<AccountBalance>,
    /// The margin balances in the account.
    pub margins: Vec<MarginBalance>,
    /// Indicates if the account state is reported by the exchange
    /// (as opposed to system-calculated).
    pub is_reported: bool,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// Additional implementation-specific account information, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<Params>,
}

impl Serialize for AccountState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let field_count = 9 + usize::from(self.info.is_some());
        let mut state = serializer.serialize_struct("AccountState", field_count)?;
        state.serialize_field("account_id", &self.account_id)?;
        state.serialize_field("account_type", &self.account_type)?;
        state.serialize_field("base_currency", &self.base_currency)?;
        if self.account_type == AccountType::Wallet {
            state.serialize_field("balances", &WalletAccountBalances::new(&self.balances))?;
        } else {
            state.serialize_field("balances", &self.balances)?;
        }
        state.serialize_field("margins", &self.margins)?;
        state.serialize_field("is_reported", &self.is_reported)?;
        state.serialize_field("event_id", &self.event_id)?;
        state.serialize_field("ts_event", &self.ts_event)?;
        state.serialize_field("ts_init", &self.ts_init)?;
        if let Some(info) = &self.info {
            state.serialize_field("info", info)?;
        }
        state.end()
    }
}

impl AccountState {
    /// Creates a new [`AccountState`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        account_type: AccountType,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        is_reported: bool,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        base_currency: Option<Currency>,
    ) -> Self {
        Self {
            account_id,
            account_type,
            base_currency,
            balances,
            margins,
            is_reported,
            event_id,
            ts_event,
            ts_init,
            info: None,
        }
    }

    /// Attaches additional implementation-specific account information to this event.
    #[must_use]
    pub fn with_info(mut self, info: Option<Params>) -> Self {
        self.info = info;
        self
    }

    /// Returns `true` if this account state has the same balances and margins as another.
    ///
    /// This compares all balances and margins for equality, returning `true` only if
    /// all balances and margins are equal. If any balance or margin is different or
    /// missing, returns `false`.
    ///
    /// # Note
    ///
    /// This method does not compare event IDs, timestamps, or other metadata - only
    /// the actual balance and margin values.
    #[must_use]
    pub fn has_same_balances_and_margins(&self, other: &Self) -> bool {
        // Quick check - if lengths differ, they can't be equal
        if self.balances.len() != other.balances.len() || self.margins.len() != other.margins.len()
        {
            return false;
        }

        // Compare balances by currency
        let self_balances: HashMap<Currency, &AccountBalance> = self
            .balances
            .iter()
            .map(|balance| (balance.currency, balance))
            .collect();

        let other_balances: HashMap<Currency, &AccountBalance> = other
            .balances
            .iter()
            .map(|balance| (balance.currency, balance))
            .collect();

        // Check if all balances are equal
        for (currency, self_balance) in &self_balances {
            match other_balances.get(currency) {
                Some(other_balance) => {
                    if self_balance != other_balance {
                        return false;
                    }
                }
                None => return false, // Currency missing in other
            }
        }

        // Compare margins by (instrument_id, currency) so that account-wide
        // entries (instrument_id = None) for different collateral currencies
        // do not collide.
        let self_margins: HashMap<(Option<InstrumentId>, Currency), &MarginBalance> = self
            .margins
            .iter()
            .map(|margin| ((margin.instrument_id, margin.currency), margin))
            .collect();

        let other_margins: HashMap<(Option<InstrumentId>, Currency), &MarginBalance> = other
            .margins
            .iter()
            .map(|margin| ((margin.instrument_id, margin.currency), margin))
            .collect();

        // Check if all margins are equal
        for (key, self_margin) in &self_margins {
            match other_margins.get(key) {
                Some(other_margin) => {
                    if self_margin != other_margin {
                        return false;
                    }
                }
                None => return false, // Entry missing in other
            }
        }

        true
    }
}

impl Display for AccountState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(account_id={}, account_type={}, base_currency={}, is_reported={}, balances=[{}], margins=[{}], event_id={})",
            stringify!(AccountState),
            self.account_id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
            self.is_reported,
            self.balances
                .iter()
                .map(|b| format!("{b}"))
                .collect::<Vec<String>>()
                .join(", "),
            self.margins
                .iter()
                .map(|m| format!("{m}"))
                .collect::<Vec<String>>()
                .join(", "),
            self.event_id
        )
    }
}

impl PartialEq for AccountState {
    fn eq(&self, other: &Self) -> bool {
        self.account_id == other.account_id
            && self.account_type == other.account_type
            && self.event_id == other.event_id
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "defi")]
    use std::process::Command;

    use indexmap::IndexMap;
    use nautilus_core::{Params, UUID4, UnixNanos};
    use rstest::rstest;
    use serde_json::json;

    use crate::{
        enums::{AccountType, CurrencyType},
        events::{
            AccountState,
            account::stubs::{cash_account_state, margin_account_state},
        },
        identifiers::{AccountId, InstrumentId},
        types::{AccountBalance, Currency, MarginBalance, Money},
    };

    #[rstest]
    fn test_equality() {
        let cash_account_state_1 = cash_account_state();
        let cash_account_state_2 = cash_account_state();
        assert_eq!(cash_account_state_1, cash_account_state_2);
    }

    #[rstest]
    fn test_display_cash_account_state(cash_account_state: AccountState) {
        let display = format!("{cash_account_state}");
        assert_eq!(
            display,
            "AccountState(account_id=SIM-001, account_type=CASH, base_currency=USD, is_reported=true, \
            balances=[AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)], \
            margins=[], event_id=16578139-a945-4b65-b46c-bc131a15d8e7)"
        );
    }

    #[rstest]
    fn test_display_margin_account_state(margin_account_state: AccountState) {
        let display = format!("{margin_account_state}");
        assert_eq!(
            display,
            "AccountState(account_id=SIM-001, account_type=MARGIN, base_currency=USD, is_reported=true, \
            balances=[AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)], \
            margins=[MarginBalance(initial=5000.00 USD, maintenance=20000.00 USD, instrument_id=BTCUSDT.COINBASE)], \
            event_id=16578139-a945-4b65-b46c-bc131a15d8e7)"
        );
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_identical() {
        let state1 = cash_account_state();
        let state2 = cash_account_state();
        assert!(state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_balance_amounts() {
        let state1 = cash_account_state();
        let mut state2 = cash_account_state();
        // Create a different balance with same currency
        let usd = Currency::USD();
        let different_balance = AccountBalance::new(
            Money::new(2_000_000.0, usd),
            Money::new(50000.0, usd),
            Money::new(1_950_000.0, usd),
        );
        state2.balances = vec![different_balance];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_balance_currencies() {
        let state1 = cash_account_state();
        let mut state2 = cash_account_state();
        // Create a balance with different currency
        let eur = Currency::EUR();
        let different_balance = AccountBalance::new(
            Money::new(1_525_000.0, eur),
            Money::new(25000.0, eur),
            Money::new(1_500_000.0, eur),
        );
        state2.balances = vec![different_balance];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_missing_balance() {
        let state1 = cash_account_state();
        let mut state2 = cash_account_state();
        // Add an additional balance to state2
        let eur = Currency::EUR();
        let additional_balance = AccountBalance::new(
            Money::new(1_000_000.0, eur),
            Money::new(0.0, eur),
            Money::new(1_000_000.0, eur),
        );
        state2.balances.push(additional_balance);
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_margin_amounts() {
        let state1 = margin_account_state();
        let mut state2 = margin_account_state();
        // Create a different margin with same instrument_id
        let usd = Currency::USD();
        let instrument_id = InstrumentId::from("BTCUSDT.COINBASE");
        let different_margin = MarginBalance::new(
            Money::new(10000.0, usd),
            Money::new(40000.0, usd),
            Some(instrument_id),
        );
        state2.margins = vec![different_margin];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_margin_instruments() {
        let state1 = margin_account_state();
        let mut state2 = margin_account_state();
        // Create a margin with different instrument_id
        let usd = Currency::USD();
        let different_instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
        let different_margin = MarginBalance::new(
            Money::new(5000.0, usd),
            Money::new(20000.0, usd),
            Some(different_instrument_id),
        );
        state2.margins = vec![different_margin];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_missing_margin() {
        let state1 = margin_account_state();
        let mut state2 = margin_account_state();
        // Add an additional margin to state2
        let usd = Currency::USD();
        let additional_instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
        let additional_margin = MarginBalance::new(
            Money::new(3000.0, usd),
            Money::new(15000.0, usd),
            Some(additional_instrument_id),
        );
        state2.margins.push(additional_margin);
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_with_empty_collections() {
        let account_id = AccountId::new("TEST-001");
        let event_id = UUID4::new();
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let state1 = AccountState::new(
            account_id,
            AccountType::Cash,
            vec![], // Empty balances
            vec![], // Empty margins
            true,
            event_id,
            ts_event,
            ts_init,
            Some(Currency::USD()),
        );

        let state2 = AccountState::new(
            account_id,
            AccountType::Cash,
            vec![], // Empty balances
            vec![], // Empty margins
            true,
            UUID4::new(),       // Different event_id
            UnixNanos::from(3), // Different timestamps
            UnixNanos::from(4),
            Some(Currency::USD()),
        );

        assert!(state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_with_multiple_balances_and_margins() {
        let account_id = AccountId::new("TEST-001");
        let event_id = UUID4::new();
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let usd = Currency::USD();
        let eur = Currency::EUR();
        let btc_instrument = InstrumentId::from("BTCUSDT.COINBASE");
        let eth_instrument = InstrumentId::from("ETHUSDT.BINANCE");

        let balances = vec![
            AccountBalance::new(
                Money::new(1_000_000.0, usd),
                Money::new(0.0, usd),
                Money::new(1_000_000.0, usd),
            ),
            AccountBalance::new(
                Money::new(500_000.0, eur),
                Money::new(10000.0, eur),
                Money::new(490_000.0, eur),
            ),
        ];

        let margins = vec![
            MarginBalance::new(
                Money::new(5000.0, usd),
                Money::new(20000.0, usd),
                Some(btc_instrument),
            ),
            MarginBalance::new(
                Money::new(3000.0, usd),
                Money::new(15000.0, usd),
                Some(eth_instrument),
            ),
        ];

        let state1 = AccountState::new(
            account_id,
            AccountType::Margin,
            balances.clone(),
            margins.clone(),
            true,
            event_id,
            ts_event,
            ts_init,
            Some(usd),
        );

        let state2 = AccountState::new(
            account_id,
            AccountType::Margin,
            balances,
            margins,
            true,
            UUID4::new(),       // Different event_id
            UnixNanos::from(3), // Different timestamps
            UnixNanos::from(4),
            Some(usd),
        );

        assert!(state1.has_same_balances_and_margins(&state2));
    }

    fn account_state_with_info() -> AccountState {
        let mut info = IndexMap::new();
        info.insert("total_wallet_balance".to_string(), json!(1525.0_f64));
        info.insert("available_balance".to_string(), json!(1500.0_f64));
        AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(Currency::USD()),
        )
        .with_info(Some(Params::from_index_map(info)))
    }

    #[rstest]
    fn test_new_defaults_info_to_none() {
        let state = cash_account_state();
        assert!(state.info.is_none());
    }

    #[rstest]
    fn test_with_info_attaches_params() {
        let state = account_state_with_info();
        let info = state.info.expect("info should be set");
        assert_eq!(info.get_f64("total_wallet_balance"), Some(1525.0));
        assert_eq!(info.get_f64("available_balance"), Some(1500.0));
        assert_eq!(info.get_f64("missing"), None);
    }

    #[rstest]
    fn test_serde_round_trips_info() {
        let state = account_state_with_info();
        let serialized = serde_json::to_string(&state).expect("serialize");
        let deserialized: AccountState = serde_json::from_str(&serialized).expect("deserialize");
        let info = deserialized.info.expect("info should round-trip");
        assert_eq!(info.get_f64("total_wallet_balance"), Some(1525.0));
        assert_eq!(info.get_f64("available_balance"), Some(1500.0));
    }

    #[rstest]
    fn test_serde_back_compatible_without_info() {
        // Serialized AccountState from before the info field existed must still
        // deserialize, defaulting info to None. Build the JSON by serializing a
        // current state and removing the info key so the format is exact.
        let state = cash_account_state();
        let mut value = serde_json::to_value(&state)
            .expect("serialize")
            .as_object()
            .cloned()
            .unwrap();
        value.remove("info");
        let deserialized: AccountState =
            serde_json::from_value(serde_json::Value::Object(value)).expect("deserialize legacy");
        assert!(deserialized.info.is_none());
    }

    #[rstest]
    #[case(AccountType::Cash, false)]
    #[case(AccountType::Margin, false)]
    #[case(AccountType::Betting, false)]
    #[case(AccountType::Wallet, true)]
    fn test_registered_account_balance_keeps_legacy_fields(
        #[case] account_type: AccountType,
        #[case] has_wallet_identity: bool,
    ) {
        let currency = Currency::USD();
        let total = Money::from("10.25 USD");
        let state = AccountState::new(
            AccountId::new("SERDE-001"),
            account_type,
            vec![AccountBalance::new(total, Money::zero(currency), total)],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );

        let value = serde_json::to_value(&state).expect("serialize account state");
        let balance = &value["balances"][0];
        let restored: AccountState =
            serde_json::from_value(value.clone()).expect("deserialize account state");

        assert_eq!(balance["currency"], "USD");
        assert_eq!(balance["total"], "10.25 USD");
        assert_eq!(balance["locked"], "0.00 USD");
        assert_eq!(balance["free"], "10.25 USD");
        assert_eq!(
            balance.get("currency_identity").is_some(),
            has_wallet_identity
        );
        assert_eq!(restored.balances[0].total.raw, total.raw);
        assert_currency_identity(restored.balances[0].currency, currency);
    }

    #[rstest]
    fn test_wallet_account_state_rejects_unaligned_raw_balance() {
        let currency = Currency::USD();
        let total = Money::from_raw(1, currency);
        let state = AccountState::new(
            AccountId::new("WALLET-SERDE-INVALID"),
            AccountType::Wallet,
            vec![AccountBalance::new(total, Money::zero(currency), total)],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );

        let error = serde_json::to_string(&state).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("is not aligned to currency precision 2"),
            "was: {error}"
        );
    }

    #[rstest]
    fn test_wallet_account_state_rejects_inconsistent_exact_balance() {
        let currency = Currency::USD();
        let total = Money::from("10.25 USD");
        let state = AccountState::new(
            AccountId::new("WALLET-SERDE-CORRUPT"),
            AccountType::Wallet,
            vec![AccountBalance::new(total, Money::zero(currency), total)],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );
        let mut value = serde_json::to_value(&state).expect("serialize account state");
        value["balances"][0]["free_minor"] = json!("999");

        let error = serde_json::from_value::<AccountState>(value).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("`total` (10.25 USD) - `locked` (0.00 USD) != `free` (9.99 USD)"),
            "was: {error}"
        );
    }

    #[rstest]
    fn test_wallet_account_state_rejects_same_code_currency_identity_mismatch() {
        let balance_currency =
            Currency::new("ENG729D", 6, 0, "Balance token", CurrencyType::Crypto);
        let money_currency = Currency::new("ENG729D", 8, 0, "Money token", CurrencyType::Crypto);
        let total = Money::from_mantissa_exponent(123_456_789, -8, money_currency);
        let balance = AccountBalance {
            currency: balance_currency,
            total,
            locked: Money::zero(money_currency),
            free: total,
        };
        let state = AccountState::new(
            AccountId::new("WALLET-SERDE-IDENTITY"),
            AccountType::Wallet,
            vec![balance],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );

        let error = serde_json::to_string(&state).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Wallet account balance currency identity ENG729D does not match"),
            "was: {error}"
        );
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_wallet_account_state_fresh_process_round_trip() {
        const PAYLOAD_ENV: &str = "NAUTILUS_WALLET_ACCOUNT_STATE_PAYLOAD";
        const SUCCESS_SENTINEL: &str = "wallet account state fresh-process assertions passed";
        const TEST_NAME: &str =
            "events::account::state::tests::test_wallet_account_state_fresh_process_round_trip";

        if let Ok(payload) = std::env::var(PAYLOAD_ENV) {
            assert!(Currency::try_from_str("ENG729A").is_none());
            assert!(Currency::try_from_str("ENG729B").is_none());

            let restored: AccountState =
                serde_json::from_str(&payload).expect("deserialize wallet account state");
            let expected = wallet_serde_balances();

            assert_eq!(restored.balances.len(), expected.len());
            for (restored, expected) in restored.balances.iter().zip(&expected) {
                assert_eq!(restored.total.raw, expected.total.raw);
                assert_eq!(restored.locked.raw, expected.locked.raw);
                assert_eq!(restored.free.raw, expected.free.raw);
                assert_currency_identity(restored.currency, expected.currency);
                assert_currency_identity(restored.total.currency, expected.total.currency);
                assert_currency_identity(restored.locked.currency, expected.locked.currency);
                assert_currency_identity(restored.free.currency, expected.free.currency);
            }
            assert!(Currency::try_from_str("ENG729A").is_none());
            assert!(Currency::try_from_str("ENG729B").is_none());
            println!("{SUCCESS_SENTINEL}");
            return;
        }

        let producer_currency = Currency::new(
            "ENG729A",
            6,
            0,
            "Producer registered token",
            CurrencyType::Crypto,
        );
        Currency::register(producer_currency, false).expect("register producer-only currency");
        let state = AccountState::new(
            AccountId::new("WALLET-SERDE-001"),
            AccountType::Wallet,
            wallet_serde_balances(),
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );
        let payload = serde_json::to_string(&state).expect("serialize wallet account state");

        let output = Command::new(std::env::current_exe().expect("current test executable"))
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(PAYLOAD_ENV, payload)
            .output()
            .expect("run fresh-process wallet round-trip");

        assert!(
            output.status.success(),
            "fresh-process wallet round-trip failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(SUCCESS_SENTINEL),
            "fresh-process wallet round-trip did not run child assertions\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(feature = "defi")]
    fn wallet_serde_balances() -> Vec<AccountBalance> {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let registered = Currency::new(
            "ENG729A",
            6,
            0,
            "Producer registered token",
            CurrencyType::Crypto,
        );
        let unregistered =
            Currency::new("ENG729B", 8, 0, "Unregistered token", CurrencyType::Crypto);
        let eth_total = Money::from_raw(1_234_567_890_123_456_789, eth);
        let registered_total = Money::from_mantissa_exponent(123_456_789, -6, registered);
        let unregistered_total = Money::from_mantissa_exponent(98_765_432, -8, unregistered);

        [eth_total, registered_total, unregistered_total]
            .into_iter()
            .map(|total| AccountBalance::new(total, Money::zero(total.currency), total))
            .collect()
    }

    fn assert_currency_identity(actual: Currency, expected: Currency) {
        assert_eq!(actual.code, expected.code);
        assert_eq!(actual.precision, expected.precision);
        assert_eq!(actual.iso4217, expected.iso4217);
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.currency_type, expected.currency_type);
    }

    #[rstest]
    fn test_info_excluded_from_equality() {
        // Equality keys on account_id, account_type, and event_id only, so a
        // differing info bag must not affect equality.
        let base = account_state_with_info();
        let mut other_info = IndexMap::new();
        other_info.insert("different".to_string(), json!(1_u64));
        let other = AccountState {
            info: Some(Params::from_index_map(other_info)),
            ..base.clone()
        };
        assert_eq!(base, other);
    }
}
