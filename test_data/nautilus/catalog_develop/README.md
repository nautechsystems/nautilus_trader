# Develop catalog fixtures

These Parquet files come from develop commit `1602043deb`. They retain its Arrow encoding and catalog paths.
`generate.rs` runs as a persistence example at that commit with `high-precision` enabled.
`128-bit/expected.json` records the original values, including nanosecond timestamps.

Migration tests copy this catalog to a temporary directory and leave these fixtures unchanged.
