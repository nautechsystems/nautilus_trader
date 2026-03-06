use std::{path::Path, time::Instant};

use nautilus_tardis::csv::stream_deltas;

fn main() {
    let test_data_path = Path::new(
        "tests/test_data/large/tardis_deribit_incremental_book_L2_2020-04-01_BTC-PERPETUAL.csv.gz",
    );

    if !test_data_path.exists() {
        eprintln!("Test data file not found: {test_data_path:?}");
        return;
    }

    println!("Running stream_deltas benchmarks...");
    println!("Test data: {test_data_path:?}");
    println!();

    // Benchmark 1: Small chunks
    println!("Small chunks (100 records per chunk)");
    let start = Instant::now();
    let stream = stream_deltas(
        test_data_path,
        100,  // Small chunk size
        None, // Auto-detect price precision
        None, // Auto-detect size precision
        None, // No instrument filter
        None, // No limit
    )
    .unwrap();
    let count: usize = stream.map(|chunk| chunk.unwrap().len()).sum();
    let duration = start.elapsed();
    println!("Processed {count} records in {duration:?}");
    println!(
        "  Rate: {:.0} records/second",
        count as f64 / duration.as_secs_f64()
    );
    println!();

    // Benchmark 2: Large chunks
    println!("Large chunks (100,000 records per chunk)");
    let start = Instant::now();
    let stream = stream_deltas(
        test_data_path,
        100_000, // Large chunk size
        None,    // Auto-detect price precision
        None,    // Auto-detect size precision
        None,    // No instrument filter
        None,    // No limit
    )
    .unwrap();
    let count: usize = stream.map(|chunk| chunk.unwrap().len()).sum();
    let duration = start.elapsed();
    println!("Processed {count} records in {duration:?}");
    println!(
        "  Rate: {:.0} records/second",
        count as f64 / duration.as_secs_f64()
    );
    println!();

    // Benchmark 3: With fixed precision
    println!("With fixed precision (1,000 records per chunk)");
    let start = Instant::now();
    let stream = stream_deltas(
        test_data_path,
        1_000,   // Medium chunk size
        Some(2), // Fixed price precision
        Some(4), // Fixed size precision
        None,    // No instrument filter
        None,    // No limit
    )
    .unwrap();
    let count: usize = stream.map(|chunk| chunk.unwrap().len()).sum();
    let duration = start.elapsed();
    println!("Processed {count} records in {duration:?}");
    println!(
        "  Rate: {:.0} records/second",
        count as f64 / duration.as_secs_f64()
    );
    println!();

    println!("Benchmarks completed!");
}
