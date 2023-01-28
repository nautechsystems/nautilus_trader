// use std::collections::BTreeMap;
// use std::io::Cursor;
// use std::slice;

// use nautilus_core::cvec::CVec;
// use nautilus_model::{
//     data::tick::TradeTick,
//     identifiers::trade_id::TradeId,
//     types::{price::Price, quantity::Quantity},
// };
// use nautilus_persistence::parquet::{
//     EncodeToChunk, GroupFilterArg, ParquetReader, ParquetType, ParquetWriter,
// };

// // comment out for faster builds
// fn main() {
//     let data = vec![
//         TradeTick {
//             instrument_id: "EUR/USD.DUKA".into(),
//             price: Price::new(1.234, 4),
//             size: Quantity::new(40.0, 0),
//             ts_event: 0,
//             ts_init: 0,
//             aggressor_side: nautilus_model::enums::AggressorSide::Buyer,
//             trade_id: TradeId::new("hey")
//         };
//         3
//     ];

//     dbg!(&data);

//     let raw_data = CVec::from(data);

//     let mut metadata: BTreeMap<String, String> = BTreeMap::new();
//     metadata.insert("instrument_id".to_string(), "EUR/USD.DUKA".to_string());
//     metadata.insert("price_precision".to_string(), "4".to_string());
//     metadata.insert("size_precision".to_string(), "4".to_string());
//     let mut writer: ParquetWriter<TradeTick, Vec<u8>> =
//         ParquetWriter::new(Vec::new(), TradeTick::encode_schema(metadata));

//     let tick_data: &[TradeTick] =
//         unsafe { slice::from_raw_parts(raw_data.ptr as *const TradeTick, 3) };
//     writer.write(tick_data).expect("unable to write");
//     let data = writer.flush();

//     let buffer = Cursor::new(data);
//     let reader: ParquetReader<TradeTick, Cursor<Vec<u8>>> =
//         ParquetReader::new(
//             buffer,
//             40,
//             GroupFilterArg::None,
//         );
//     for data in reader {
//         dbg!(data);
//     }
// }

fn main() {}
