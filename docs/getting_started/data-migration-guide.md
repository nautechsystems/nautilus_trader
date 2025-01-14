Nautilus defines an internal data format specified in the nautilus_model crate. These models are serialized into record batches and written to parquet files. Nautilus backtesting works best with these parquet files. However, migrating the data model between schema changes or to custom schemas can be difficult. `to_json` and `to_parquet` are two applications in the nautilus_persistence crate that can help with this.

* `to_json` reads a parquet file and serializes the data into json objects. It also stores the metadata associated with the parquet file.
* `to_parquet` reads the json file and the metadata and writes the data back into parquet format.

A typical schema migration exercise might look like this.

```
# Checkout commit with current schema
cargo run bin --to_json <parquet-path>  # creates <parquet-path>.json and <parquet-path>.metadata.json
# Checkout commit with new schema
cargo run bin --to_parquet <json-path>  # reads <json-path> and <json-path>.metadata.json and writes to <json-path>.parquet with new schema
```

Before an actual migration, it is recommended that you do a mock migration by converting the data between from current schema to json and back.
