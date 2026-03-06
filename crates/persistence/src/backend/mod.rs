//! Provides an Apache Parquet backend powered by [DataFusion](https://arrow.apache.org/datafusion).

pub mod binary_heap;
pub mod catalog;
pub mod catalog_operations;
pub mod compare;
pub mod feather;
pub mod kmerge_batch;
pub mod session;
