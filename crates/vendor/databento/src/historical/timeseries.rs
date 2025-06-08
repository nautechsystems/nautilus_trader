//! The historical timeseries API.

use std::{num::NonZeroU64, path::PathBuf};

// Re-export because it's returned.
pub use dbn::decode::AsyncDbnDecoder;
use dbn::{encode::AsyncDbnEncoder, Compression, Encoding, SType, Schema, VersionUpgradePolicy};
use futures::{Stream, TryStreamExt};
use reqwest::{header::ACCEPT, RequestBuilder};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
};
use tokio_util::{bytes::Bytes, io::StreamReader};
use typed_builder::TypedBuilder;

use super::{check_http_error, DateTimeRange};
use crate::Symbols;

/// A client for the timeseries group of Historical API endpoints.
#[derive(Debug)]
pub struct TimeseriesClient<'a> {
    pub(crate) inner: &'a mut super::Client,
}

impl TimeseriesClient<'_> {
    /// Makes a streaming request for timeseries data from Databento.
    ///
    /// This method returns a stream decoder. For larger requests, consider using
    /// [`BatchClient::submit_job()`](super::batch::BatchClient::submit_job()).
    ///
    /// <div class="warning">
    /// Calling this method will incur a cost.
    /// </div>
    ///
    /// # Errors
    /// This function returns an error when it fails to communicate with the Databento API
    /// or the API indicates there's an issue with the request.
    pub async fn get_range(
        &mut self,
        params: &GetRangeParams,
    ) -> crate::Result<AsyncDbnDecoder<impl AsyncReadExt>> {
        let reader = self
            .get_range_impl(
                &params.dataset,
                params.schema,
                params.stype_in,
                params.stype_out,
                &params.symbols,
                &params.date_time_range,
                params.limit,
            )
            .await?;
        Ok(
            AsyncDbnDecoder::with_upgrade_policy(zstd_decoder(reader), params.upgrade_policy)
                .await?,
        )
    }

    /// Makes a streaming request for timeseries data from Databento.
    ///
    /// This method returns a stream decoder. For larger requests, consider using
    /// [`BatchClient::submit_job()`](super::batch::BatchClient::submit_job()).
    ///
    /// <div class="warning">
    /// Calling this method will incur a cost.
    /// </div>
    ///
    /// # Errors
    /// This function returns an error when it fails to communicate with the Databento API
    /// or the API indicates there's an issue with the request. An error will also be returned
    /// if it fails to create a new file at `path`.
    pub async fn get_range_to_file(
        &mut self,
        params: &GetRangeToFileParams,
    ) -> crate::Result<AsyncDbnDecoder<impl AsyncReadExt>> {
        let reader = self
            .get_range_impl(
                &params.dataset,
                params.schema,
                params.stype_in,
                params.stype_out,
                &params.symbols,
                &params.date_time_range,
                params.limit,
            )
            .await?;
        let mut http_decoder =
            AsyncDbnDecoder::with_upgrade_policy(zstd_decoder(reader), params.upgrade_policy)
                .await?;
        let file = BufWriter::new(File::create(&params.path).await?);
        let mut encoder = AsyncDbnEncoder::with_zstd(file, http_decoder.metadata()).await?;
        while let Some(rec_ref) = http_decoder.decode_record_ref().await? {
            encoder.encode_record_ref(rec_ref).await?;
        }
        encoder.get_mut().shutdown().await?;
        Ok(AsyncDbnDecoder::with_upgrade_policy(
            zstd_decoder(BufReader::new(File::open(&params.path).await?)),
            // Applied upgrade policy during initial decoding
            VersionUpgradePolicy::AsIs,
        )
        .await?)
    }

    #[allow(clippy::too_many_arguments)] // private method
    async fn get_range_impl(
        &mut self,
        dataset: &str,
        schema: Schema,
        stype_in: SType,
        stype_out: SType,
        symbols: &Symbols,
        date_time_range: &DateTimeRange,
        limit: Option<NonZeroU64>,
    ) -> crate::Result<StreamReader<impl Stream<Item = std::io::Result<Bytes>>, Bytes>> {
        let mut form = vec![
            ("dataset", dataset.to_owned()),
            ("schema", schema.to_string()),
            ("encoding", Encoding::Dbn.to_string()),
            ("compression", Compression::ZStd.to_string()),
            ("stype_in", stype_in.to_string()),
            ("stype_out", stype_out.to_string()),
            ("symbols", symbols.to_api_string()),
        ];
        date_time_range.add_to_form(&mut form);
        if let Some(limit) = limit {
            form.push(("limit", limit.to_string()));
        }
        let resp = self
            .post("get_range")?
            // unlike almost every other request, it's not JSON
            .header(ACCEPT, "application/octet-stream")
            .form(&form)
            .send()
            .await?;
        let stream = check_http_error(resp)
            .await?
            .error_for_status()?
            .bytes_stream()
            .map_err(std::io::Error::other);
        Ok(tokio_util::io::StreamReader::new(stream))
    }

    fn post(&mut self, slug: &str) -> crate::Result<RequestBuilder> {
        self.inner.post(&format!("timeseries.{slug}"))
    }
}

/// The parameters for [`TimeseriesClient::get_range()`]. Use
/// [`GetRangeParams::builder()`] to get a builder type with all the preset defaults.
#[derive(Debug, Clone, TypedBuilder, PartialEq, Eq)]
pub struct GetRangeParams {
    /// The dataset code.
    #[builder(setter(transform = |dt: impl ToString| dt.to_string()))]
    pub dataset: String,
    /// The symbols to filter for.
    #[builder(setter(into))]
    pub symbols: Symbols,
    /// The data record schema.
    pub schema: Schema,
    /// The date time request range.
    /// Filters on `ts_recv` if it exists in the schema, otherwise `ts_event`.
    #[builder(setter(into))]
    pub date_time_range: DateTimeRange,
    /// The symbology type of the input `symbols`. Defaults to
    /// [`RawSymbol`](dbn::enums::SType::RawSymbol).
    #[builder(default = SType::RawSymbol)]
    pub stype_in: SType,
    /// The symbology type of the output `symbols`. Defaults to
    /// [`InstrumentId`](dbn::enums::SType::InstrumentId).
    #[builder(default = SType::InstrumentId)]
    pub stype_out: SType,
    /// The optional maximum number of records to return. Defaults to no limit.
    #[builder(default)]
    pub limit: Option<NonZeroU64>,
    /// How to decode DBN from prior versions. Defaults to upgrade.
    #[builder(default)]
    pub upgrade_policy: VersionUpgradePolicy,
}

/// The parameters for [`TimeseriesClient::get_range_to_file()`]. Use
/// [`GetRangeToFileParams::builder()`] to get a builder type with all the preset defaults.
#[derive(Debug, Clone, TypedBuilder, PartialEq, Eq)]
pub struct GetRangeToFileParams {
    /// The dataset code.
    #[builder(setter(transform = |dt: impl ToString| dt.to_string()))]
    pub dataset: String,
    /// The symbols to filter for.
    #[builder(setter(into))]
    pub symbols: Symbols,
    /// The data record schema.
    pub schema: Schema,
    /// The date time request range.
    /// Filters on `ts_recv` if it exists in the schema, otherwise `ts_event`.
    #[builder(setter(into))]
    pub date_time_range: DateTimeRange,
    /// The symbology type of the input `symbols`. Defaults to
    /// [`RawSymbol`](dbn::enums::SType::RawSymbol).
    #[builder(default = SType::RawSymbol)]
    pub stype_in: SType,
    /// The symbology type of the output `symbols`. Defaults to
    /// [`InstrumentId`](dbn::enums::SType::InstrumentId).
    #[builder(default = SType::InstrumentId)]
    pub stype_out: SType,
    /// The optional maximum number of records to return. Defaults to no limit.
    #[builder(default)]
    pub limit: Option<NonZeroU64>,
    /// How to decode DBN from prior versions. Defaults to upgrade.
    #[builder(default)]
    pub upgrade_policy: VersionUpgradePolicy,
    /// The file path to persist the stream data to.
    #[builder(default, setter(transform = |p: impl Into<PathBuf>| p.into()))]
    pub path: PathBuf,
}

impl From<GetRangeToFileParams> for GetRangeParams {
    fn from(value: GetRangeToFileParams) -> Self {
        Self {
            dataset: value.dataset,
            symbols: value.symbols,
            schema: value.schema,
            date_time_range: value.date_time_range,
            stype_in: value.stype_in,
            stype_out: value.stype_out,
            limit: value.limit,
            upgrade_policy: value.upgrade_policy,
        }
    }
}

impl GetRangeParams {
    /// Converts these parameters into a request that will be persisted to a file
    /// at `path`. Used in conjunction with [`TimeseriesClient::get_range_to_file()``].
    pub fn with_path(self, path: impl Into<PathBuf>) -> GetRangeToFileParams {
        GetRangeToFileParams {
            dataset: self.dataset,
            symbols: self.symbols,
            schema: self.schema,
            date_time_range: self.date_time_range,
            stype_in: self.stype_in,
            stype_out: self.stype_out,
            limit: self.limit,
            upgrade_policy: self.upgrade_policy,
            path: path.into(),
        }
    }
}

fn zstd_decoder<R>(reader: R) -> async_compression::tokio::bufread::ZstdDecoder<R>
where
    R: tokio::io::AsyncBufReadExt + Unpin,
{
    let mut zstd_decoder = async_compression::tokio::bufread::ZstdDecoder::new(reader);
    // explicitly enable decoding multiple frames
    zstd_decoder.multiple_members(true);
    zstd_decoder
}

#[cfg(test)]
mod tests {
    use dbn::{record::TradeMsg, Dataset};
    use reqwest::StatusCode;
    use rstest::*;
    use time::macros::datetime;
    use wiremock::{
        matchers::{basic_auth, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use super::*;
    use crate::{
        body_contains,
        historical::{HistoricalGateway, API_VERSION},
        zst_test_data_path, HistoricalClient,
    };

    const API_KEY: &str = "test-API";

    #[rstest]
    #[case(VersionUpgradePolicy::AsIs, 1)]
    #[case(VersionUpgradePolicy::UpgradeToV2, 2)]
    #[case(VersionUpgradePolicy::UpgradeToV3, 3)]
    #[tokio::test]
    async fn test_get_range(#[case] upgrade_policy: VersionUpgradePolicy, #[case] exp_version: u8) {
        const START: time::OffsetDateTime = datetime!(2023 - 06 - 14 00:00 UTC);
        const END: time::OffsetDateTime = datetime!(2023 - 06 - 17 00:00 UTC);
        const SCHEMA: Schema = Schema::Trades;

        let mock_server = MockServer::start().await;
        let bytes = tokio::fs::read(zst_test_data_path(SCHEMA)).await.unwrap();
        Mock::given(method("POST"))
            .and(basic_auth(API_KEY, ""))
            .and(path(format!("/v{API_VERSION}/timeseries.get_range")))
            .and(body_contains("dataset", "XNAS.ITCH"))
            .and(body_contains("schema", "trades"))
            .and(body_contains("symbols", "SPOT%2CAAPL"))
            .and(body_contains(
                "start",
                START.unix_timestamp_nanos().to_string(),
            ))
            .and(body_contains("end", END.unix_timestamp_nanos().to_string()))
            // // default
            .and(body_contains("stype_in", "raw_symbol"))
            .and(body_contains("stype_out", "instrument_id"))
            .respond_with(ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_bytes(bytes))
            .mount(&mock_server)
            .await;
        let mut target = HistoricalClient::with_url(
            mock_server.uri(),
            API_KEY.to_owned(),
            HistoricalGateway::Bo1,
        )
        .unwrap();
        let mut decoder = target
            .timeseries()
            .get_range(
                &GetRangeParams::builder()
                    .dataset(dbn::Dataset::XnasItch)
                    .schema(SCHEMA)
                    .symbols(vec!["SPOT", "AAPL"])
                    .date_time_range((START, END))
                    .upgrade_policy(upgrade_policy)
                    .build(),
            )
            .await
            .unwrap();
        let metadata = decoder.metadata();
        assert_eq!(metadata.schema.unwrap(), SCHEMA);
        assert_eq!(metadata.version, exp_version);
        // Two records
        decoder.decode_record::<TradeMsg>().await.unwrap().unwrap();
        decoder.decode_record::<TradeMsg>().await.unwrap().unwrap();
        assert!(decoder.decode_record::<TradeMsg>().await.unwrap().is_none());
    }

    #[rstest]
    #[case(VersionUpgradePolicy::AsIs, 1)]
    #[case(VersionUpgradePolicy::UpgradeToV2, 2)]
    #[case(VersionUpgradePolicy::UpgradeToV3, 3)]
    #[tokio::test]
    async fn test_get_range_to_file(
        #[case] upgrade_policy: VersionUpgradePolicy,
        #[case] exp_version: u8,
    ) {
        const START: time::OffsetDateTime = datetime!(2024 - 05 - 17 00:00 UTC);
        const END: time::OffsetDateTime = datetime!(2024 - 05 - 18 00:00 UTC);
        const SCHEMA: Schema = Schema::Trades;
        const DATASET: &str = Dataset::IfeuImpact.as_str();

        let mock_server = MockServer::start().await;
        let temp_dir = tempfile::TempDir::new().unwrap();
        let bytes = tokio::fs::read(zst_test_data_path(SCHEMA)).await.unwrap();
        Mock::given(method("POST"))
            .and(basic_auth(API_KEY, ""))
            .and(path(format!("/v{API_VERSION}/timeseries.get_range")))
            .and(body_contains("dataset", DATASET))
            .and(body_contains("schema", "trades"))
            .and(body_contains("symbols", "BRN.FUT"))
            .and(body_contains(
                "start",
                START.unix_timestamp_nanos().to_string(),
            ))
            .and(body_contains("end", END.unix_timestamp_nanos().to_string()))
            // // default
            .and(body_contains("stype_in", "parent"))
            .and(body_contains("stype_out", "instrument_id"))
            .respond_with(ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_bytes(bytes))
            .mount(&mock_server)
            .await;
        let mut target = HistoricalClient::with_url(
            mock_server.uri(),
            API_KEY.to_owned(),
            HistoricalGateway::Bo1,
        )
        .unwrap();
        let path = temp_dir.path().join("test.dbn.zst");
        let mut decoder = target
            .timeseries()
            .get_range_to_file(
                &GetRangeToFileParams::builder()
                    .dataset(DATASET)
                    .schema(SCHEMA)
                    .symbols(vec!["BRN.FUT"])
                    .stype_in(SType::Parent)
                    .date_time_range((START, END))
                    .path(path.clone())
                    .upgrade_policy(upgrade_policy)
                    .build(),
            )
            .await
            .unwrap();
        let metadata = decoder.metadata();
        assert_eq!(metadata.schema.unwrap(), SCHEMA);
        assert_eq!(metadata.version, exp_version);
        // Two records
        decoder.decode_record::<TradeMsg>().await.unwrap().unwrap();
        decoder.decode_record::<TradeMsg>().await.unwrap().unwrap();
        assert!(decoder.decode_record::<TradeMsg>().await.unwrap().is_none());
    }
}
