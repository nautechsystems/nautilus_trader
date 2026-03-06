use nautilus_model::{data::BarType, identifiers::InstrumentId};
use nautilus_okx::{common::enums::OKXInstrumentType, http::client::OKXHttpClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let client = OKXHttpClient::from_env().unwrap();

    // Request instruments
    let inst_type = OKXInstrumentType::Swap;
    let (instruments, _inst_id_codes) = client.request_instruments(inst_type, None).await?;
    client.cache_instruments(instruments);

    let inst_type = OKXInstrumentType::Spot;
    let (instruments, _inst_id_codes) = client.request_instruments(inst_type, None).await?;
    client.cache_instruments(instruments);

    // Request mark price
    let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");

    let resp = client.request_mark_price(instrument_id).await;
    match resp {
        Ok(resp) => log::debug!("{resp:?}"),
        Err(e) => log::error!("{e:?}"),
    }

    // Request index price
    let instrument_id = InstrumentId::from("BTC-USDT.OKX");

    let resp = client.request_index_price(instrument_id).await;
    match resp {
        Ok(resp) => log::debug!("{resp:?}"),
        Err(e) => log::error!("{e:?}"),
    }

    // Request trades
    let resp = client.request_trades(instrument_id, None, None, None).await;
    match resp {
        Ok(resp) => log::debug!("{resp:?}"),
        Err(e) => log::error!("{e:?}"),
    }

    // Request bars
    let bar_type = BarType::from("BTC-USDT-SWAP.OKX-1-MINUTE-LAST-EXTERNAL");

    let resp = client.request_bars(bar_type, None, None, None).await;
    match resp {
        Ok(resp) => log::debug!("{resp:?}"),
        Err(e) => log::error!("{e:?}"),
    }

    // let params = GetPositionTiersParamsBuilder::default()
    //     .instrument_type(OKXInstrumentType::Swap)
    //     .trade_mode(OKXTradeMode::Isolated)
    //     .instrument_family("BTC-USD")
    //     .build()?;
    // match client.http_get_position_tiers(params).await {
    //     Ok(resp) => log::debug!("{:?}", resp),
    //     Err(e) => log::error!("{e:?}"),
    // }
    //

    //
    // let params = GetPositionsParamsBuilder::default()
    //     .instrument_type(OKXInstrumentType::Swap)
    //     .build()?;
    // match client.http_get_positions(params).await {
    //     Ok(resp) => log::debug!("{:?}", resp),
    //     Err(e) => log::error!("{e:?}"),
    // }
    //
    // let params = GetPositionsHistoryParamsBuilder::default()
    //     .instrument_type(OKXInstrumentType::Swap)
    //     .build()?;
    // match client.http_get_position_history(params).await {
    //     Ok(resp) => log::debug!("{:?}", resp),
    //     Err(e) => log::error!("{e:?}"),
    // }

    Ok(())
}
