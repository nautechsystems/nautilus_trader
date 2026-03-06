use nautilus_core::UUID4;
use rstest::fixture;

use crate::{
    enums::AccountType,
    events::AccountState,
    identifiers::stubs::{account_id, uuid4},
    types::{
        AccountBalance, Currency, Money,
        stubs::{stub_account_balance, stub_margin_balance},
    },
};

#[fixture]
pub fn cash_account_state() -> AccountState {
    AccountState::new(
        account_id(),
        AccountType::Cash,
        vec![stub_account_balance()],
        vec![],
        true,
        uuid4(),
        0.into(),
        0.into(),
        Some(Currency::USD()),
    )
}

#[fixture]
pub fn cash_account_state_million_usd(
    #[default("1000000 USD")] total: &str,
    #[default("0 USD")] locked: &str,
    #[default("1000000 USD")] free: &str,
) -> AccountState {
    AccountState::new(
        account_id(),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from(total),
            Money::from(locked),
            Money::from(free),
        )],
        vec![],
        true,
        UUID4::new(),
        0.into(),
        0.into(),
        Some(Currency::USD()),
    )
}

#[fixture]
pub fn cash_account_state_million_usdt() -> AccountState {
    AccountState::new(
        account_id(),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000000 USD"),
            Money::from("0 USD"),
            Money::from("1000000 USD"),
        )],
        vec![],
        true,
        uuid4(),
        0.into(),
        0.into(),
        Some(Currency::USD()),
    )
}

#[fixture]
pub fn cash_account_state_multi() -> AccountState {
    let btc_account_balance = AccountBalance::new(
        Money::from("10 BTC"),
        Money::from("0 BTC"),
        Money::from("10 BTC"),
    );
    let eth_account_balance = AccountBalance::new(
        Money::from("20 ETH"),
        Money::from("0 ETH"),
        Money::from("20 ETH"),
    );
    AccountState::new(
        account_id(),
        AccountType::Cash,
        vec![btc_account_balance, eth_account_balance],
        vec![],
        true,
        uuid4(),
        0.into(),
        0.into(),
        None, // multi cash account
    )
}

#[fixture]
pub fn cash_account_state_multi_changed_btc() -> AccountState {
    let btc_account_balance = AccountBalance::new(
        Money::from("9 BTC"),
        Money::from("0.5 BTC"),
        Money::from("8.5 BTC"),
    );
    let eth_account_balance = AccountBalance::new(
        Money::from("20 ETH"),
        Money::from("0 ETH"),
        Money::from("20 ETH"),
    );
    AccountState::new(
        account_id(),
        AccountType::Cash,
        vec![btc_account_balance, eth_account_balance],
        vec![],
        true,
        uuid4(),
        0.into(),
        0.into(),
        None, // multi cash account
    )
}

#[fixture]
pub fn margin_account_state() -> AccountState {
    AccountState::new(
        account_id(),
        AccountType::Margin,
        vec![stub_account_balance()],
        vec![stub_margin_balance()],
        true,
        uuid4(),
        0.into(),
        0.into(),
        Some(Currency::USD()),
    )
}
