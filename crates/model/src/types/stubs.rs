use rstest::fixture;

use crate::{
    identifiers::stubs::instrument_id_btc_usdt,
    types::{AccountBalance, MarginBalance, Money},
};

#[fixture]
pub fn stub_account_balance() -> AccountBalance {
    let total = Money::from("1525000 USD");
    let locked = Money::from("25000 USD");
    let free = Money::from("1500000 USD");
    AccountBalance::new(total, locked, free)
}

#[fixture]
pub fn stub_margin_balance() -> MarginBalance {
    let initial = Money::from("5000 USD");
    let maintenance = Money::from("20000 USD");
    let instrument = instrument_id_btc_usdt();
    MarginBalance::new(initial, maintenance, instrument)
}
