use std::str::FromStr;

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::AccountType, events::AccountState, identifiers::AccountId, types::Currency,
};
use sqlx::{FromRow, Row, postgres::PgRow};

#[derive(Debug)]
pub struct AccountEventModel(pub AccountState);

impl<'r> FromRow<'r, PgRow> for AccountEventModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let account_id = row.try_get::<&str, _>("account_id").map(AccountId::from)?;
        let account_type = AccountType::from_str(row.try_get::<&str, _>("kind")?).unwrap();
        let is_reported = row.try_get::<bool, _>("is_reported")?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let base_currency = row
            .try_get::<Option<&str>, _>("base_currency")
            .map(|res| res.map(Currency::from))?;
        let balances: serde_json::Value = row.try_get("balances")?;
        let margins: serde_json::Value = row.try_get("margins")?;
        let account_event = AccountState::new(
            account_id,
            account_type,
            serde_json::from_value(balances).unwrap(),
            serde_json::from_value(margins).unwrap(),
            is_reported,
            event_id,
            ts_event,
            ts_init,
            base_currency,
        );
        Ok(Self(account_event))
    }
}
