use nautilus_core::cvec::CVec;
use nautilus_persistence::parquet::{
    parquet_reader_drop, parquet_reader_drop_chunk, parquet_reader_new, parquet_reader_next_chunk,
    ParquetReaderType,
};
use pyo3::{prelude::*, types::*};

#[test]
fn test_parquet_reader() {
    pyo3::prepare_freethreaded_python();

    let file_path = "../../tests/test_kit/data/quote_tick_data.parquet";

    // return an opaque reader pointer
    let file_path = Python::with_gil(|py| PyString::new(py, file_path).into());

    let reader = unsafe { parquet_reader_new(file_path, ParquetReaderType::QuoteTick) };

    let mut total = 0;
    let mut chunk = CVec::default();
    unsafe {
        loop {
            chunk = parquet_reader_next_chunk(reader, ParquetReaderType::QuoteTick);
            if chunk.len == 0 {
                parquet_reader_drop_chunk(chunk, ParquetReaderType::QuoteTick);
                break;
            } else {
                total += chunk.len;
                parquet_reader_drop_chunk(chunk, ParquetReaderType::QuoteTick);
            }
        }
    }

    unsafe {
        parquet_reader_drop(reader, ParquetReaderType::QuoteTick);
    }

    assert_eq!(total, 9500);
}
