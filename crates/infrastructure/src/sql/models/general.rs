use nautilus_model::identifiers::{ClientId, ClientOrderId};
use sqlx::{Error, FromRow, Row, postgres::PgRow};

#[derive(Debug, sqlx::FromRow)]
pub struct GeneralRow {
    pub id: String,
    pub value: Vec<u8>,
}

#[derive(Debug)]
pub struct OrderEventOrderClientIdCombination {
    pub client_order_id: ClientOrderId,
    pub client_id: ClientId,
}

impl<'r> FromRow<'r, PgRow> for OrderEventOrderClientIdCombination {
    fn from_row(row: &'r PgRow) -> Result<Self, Error> {
        let client_order_id = row
            .try_get::<&str, _>("client_order_id")
            .map(ClientOrderId::from)
            .unwrap();
        let client_id = row
            .try_get::<&str, _>("client_id")
            .map(ClientId::from)
            .unwrap();
        Ok(Self {
            client_order_id,
            client_id,
        })
    }
}
