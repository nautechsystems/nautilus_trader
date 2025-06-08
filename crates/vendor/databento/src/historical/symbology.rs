//! The historical symbology API.

use std::{collections::HashMap, sync::Arc};

use dbn::{MappingInterval, Metadata, SType, TsSymbolMap};
use reqwest::RequestBuilder;
use serde::Deserialize;
use typed_builder::TypedBuilder;

use super::{handle_response, timeseries, DateRange, DateTimeRange};
use crate::Symbols;

/// A client for the symbology group of Historical API endpoints.
#[derive(Debug)]
pub struct SymbologyClient<'a> {
    pub(crate) inner: &'a mut super::Client,
}

impl SymbologyClient<'_> {
    /// Resolves a list of symbols from an input symbology type to an output one.
    ///
    /// For example, resolves a raw symbol to an instrument ID: `ESM2` → `3403`.
    ///
    /// # Errors
    /// This function returns an error when it fails to communicate with the Databento API
    /// or the API indicates there's an issue with the request.
    pub async fn resolve(&mut self, params: &ResolveParams) -> crate::Result<Resolution> {
        let mut form = vec![
            ("dataset", params.dataset.to_string()),
            ("stype_in", params.stype_in.to_string()),
            ("stype_out", params.stype_out.to_string()),
            ("symbols", params.symbols.to_api_string()),
        ];
        params.date_range.add_to_form(&mut form);
        let resp = self.post("resolve")?.form(&form).send().await?;
        let ResolutionResp {
            mappings,
            partial,
            not_found,
        } = handle_response(resp).await?;
        Ok(Resolution {
            mappings,
            partial,
            not_found,
            stype_in: params.stype_in,
            stype_out: params.stype_out,
        })
    }

    fn post(&mut self, slug: &str) -> crate::Result<RequestBuilder> {
        self.inner.post(&format!("symbology.{slug}"))
    }
}

/// The parameters for [`SymbologyClient::resolve()`]. Use [`ResolveParams::builder()`]
/// to get a builder type with all the preset defaults.
#[derive(Debug, Clone, TypedBuilder, PartialEq, Eq)]
pub struct ResolveParams {
    /// The dataset code.
    #[builder(setter(transform = |dt: impl ToString| dt.to_string()))]
    pub dataset: String,
    /// The symbols to resolve.
    #[builder(setter(into))]
    pub symbols: Symbols,
    /// The symbology type of the input `symbols`. Defaults to
    /// [`RawSymbol`](dbn::enums::SType::RawSymbol).
    #[builder(default = SType::RawSymbol)]
    pub stype_in: SType,
    /// The symbology type of the output `symbols`. Defaults to
    /// [`InstrumentId`](dbn::enums::SType::InstrumentId).
    #[builder(default = SType::InstrumentId)]
    pub stype_out: SType,
    /// The date range of the resolution.
    #[builder(setter(into))]
    pub date_range: DateRange,
}

/// Primarily intended for requesting mappings for historical ALL_SYMBOLS requests,
/// which currently don't return mappings on their own.
impl TryFrom<Metadata> for ResolveParams {
    type Error = crate::Error;

    fn try_from(metadata: Metadata) -> Result<Self, Self::Error> {
        let stype_in = metadata
            .stype_in
            .ok_or_else(|| crate::Error::bad_arg("metadata", "stype_in must be Some value"))?;
        let end = metadata
            .end()
            .ok_or_else(|| crate::Error::bad_arg("metadata", "end must be Some value"))?;
        let dt_range = DateTimeRange::from((metadata.start(), end));
        Ok(Self {
            dataset: metadata.dataset,
            symbols: Symbols::Symbols(metadata.symbols),
            stype_in,
            stype_out: metadata.stype_out,
            date_range: DateRange::from(dt_range),
        })
    }
}

impl From<timeseries::GetRangeParams> for ResolveParams {
    fn from(get_range_params: timeseries::GetRangeParams) -> Self {
        Self {
            dataset: get_range_params.dataset,
            symbols: get_range_params.symbols,
            stype_in: get_range_params.stype_in,
            stype_out: get_range_params.stype_out,
            date_range: DateRange::from(get_range_params.date_time_range),
        }
    }
}

impl From<timeseries::GetRangeToFileParams> for ResolveParams {
    fn from(get_range_to_file_params: timeseries::GetRangeToFileParams) -> Self {
        Self::from(timeseries::GetRangeParams::from(get_range_to_file_params))
    }
}

/// A symbology resolution from one symbology type to another.
#[derive(Debug, Clone)]
pub struct Resolution {
    /// A mapping from input symbol to a list of resolved symbols in the output
    /// symbology.
    pub mappings: HashMap<String, Vec<MappingInterval>>,
    /// A list of symbols that were resolved for part, but not all of the date range
    /// from the request.
    pub partial: Vec<String>,
    /// A list of symbols that were not resolved.
    pub not_found: Vec<String>,
    /// The input symbology type.
    pub stype_in: SType,
    /// The output symbology type.
    pub stype_out: SType,
}

impl Resolution {
    /// Creates a symbology mapping from instrument ID and date to text symbol.
    ///
    /// # Errors
    /// This function returns an error if it's unable to parse a symbol into an
    /// instrument ID.
    pub fn symbol_map(&self) -> crate::Result<TsSymbolMap> {
        let mut map = TsSymbolMap::new();
        if self.stype_in == SType::InstrumentId {
            for (iid, intervals) in self.mappings.iter() {
                let iid = iid.parse().map_err(|_| {
                    crate::Error::internal(format!("Unable to parse '{iid}' to an instrument ID",))
                })?;
                for interval in intervals {
                    map.insert(
                        iid,
                        interval.start_date,
                        interval.end_date,
                        Arc::new(interval.symbol.clone()),
                    )?;
                }
            }
        } else {
            for (raw_symbol, intervals) in self.mappings.iter() {
                let raw_symbol = Arc::new(raw_symbol.clone());
                for interval in intervals {
                    let iid = interval.symbol.parse().map_err(|_| {
                        crate::Error::internal(format!(
                            "Unable to parse '{}' to an instrument ID",
                            interval.symbol
                        ))
                    })?;
                    map.insert(
                        iid,
                        interval.start_date,
                        interval.end_date,
                        raw_symbol.clone(),
                    )?;
                }
            }
        }
        Ok(map)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ResolutionResp {
    #[serde(rename = "result")]
    pub mappings: HashMap<String, Vec<MappingInterval>>,
    pub partial: Vec<String>,
    pub not_found: Vec<String>,
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;
    use time::macros::date;
    use wiremock::{
        matchers::{basic_auth, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use super::*;
    use crate::{
        body_contains,
        historical::{HistoricalGateway, API_VERSION},
        HistoricalClient,
    };

    const API_KEY: &str = "test-API";

    #[tokio::test]
    async fn test_resolve() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(basic_auth(API_KEY, ""))
            .and(path(format!("/v{API_VERSION}/symbology.resolve")))
            .and(body_contains("dataset", "GLBX.MDP3"))
            .and(body_contains("symbols", "ES.c.0%2CES.d.0"))
            .and(body_contains("stype_in", "continuous"))
            // default
            .and(body_contains("stype_out", "instrument_id"))
            .and(body_contains("start_date", "2023-06-14"))
            .and(body_contains("end_date", "2023-06-17"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_json(json!({
                    "result": {
                        "ES.c.0": [
                            {
                                "d0": "2023-06-14",
                                "d1": "2023-06-15",
                                "s": "10245"
                            },
                            {
                                "d0": "2023-06-15",
                                "d1": "2023-06-16",
                                "s": "10248"
                            }
                        ]
                    },
                    "partial": [],
                    "not_found": ["ES.d.0"]
                })),
            )
            .mount(&mock_server)
            .await;
        let mut target = HistoricalClient::with_url(
            mock_server.uri(),
            API_KEY.to_owned(),
            HistoricalGateway::Bo1,
        )
        .unwrap();
        let res = target
            .symbology()
            .resolve(
                &ResolveParams::builder()
                    .dataset(dbn::Dataset::GlbxMdp3)
                    .symbols(vec!["ES.c.0", "ES.d.0"])
                    .stype_in(SType::Continuous)
                    .date_range((date!(2023 - 06 - 14), date!(2023 - 06 - 17)))
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(
            *res.mappings.get("ES.c.0").unwrap(),
            vec![
                MappingInterval {
                    start_date: time::macros::date!(2023 - 06 - 14),
                    end_date: time::macros::date!(2023 - 06 - 15),
                    symbol: "10245".to_owned()
                },
                MappingInterval {
                    start_date: time::macros::date!(2023 - 06 - 15),
                    end_date: time::macros::date!(2023 - 06 - 16),
                    symbol: "10248".to_owned()
                },
            ]
        );
        assert!(res.partial.is_empty());
        assert_eq!(res.not_found, vec!["ES.d.0"]);
    }
}
