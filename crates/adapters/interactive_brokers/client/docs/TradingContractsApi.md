# \TradingContractsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**iserver_contract_conid_algos_get**](TradingContractsApi.md#iserver_contract_conid_algos_get) | **GET** /iserver/contract/{conid}/algos | Search Algo Params By Contract ID
[**iserver_contract_conid_info_and_rules_get**](TradingContractsApi.md#iserver_contract_conid_info_and_rules_get) | **GET** /iserver/contract/{conid}/info-and-rules | Contract Information And Rules By Contract ID
[**iserver_contract_conid_info_get**](TradingContractsApi.md#iserver_contract_conid_info_get) | **GET** /iserver/contract/{conid}/info | Contract Information By Contract ID
[**iserver_contract_rules_post**](TradingContractsApi.md#iserver_contract_rules_post) | **POST** /iserver/contract/rules | Search Contract Rules
[**iserver_currency_pairs_get**](TradingContractsApi.md#iserver_currency_pairs_get) | **GET** /iserver/currency/pairs |
[**iserver_exchangerate_get**](TradingContractsApi.md#iserver_exchangerate_get) | **GET** /iserver/exchangerate | Currency Exchange Rate
[**iserver_secdef_bond_filters_get**](TradingContractsApi.md#iserver_secdef_bond_filters_get) | **GET** /iserver/secdef/bond-filters | Search Bond Filter Information
[**iserver_secdef_info_get**](TradingContractsApi.md#iserver_secdef_info_get) | **GET** /iserver/secdef/info | SecDef Info
[**iserver_secdef_search_get**](TradingContractsApi.md#iserver_secdef_search_get) | **GET** /iserver/secdef/search | Returns A List Of Contracts Based On Symbol.
[**iserver_secdef_search_post**](TradingContractsApi.md#iserver_secdef_search_post) | **POST** /iserver/secdef/search | Returns A List Of Contracts Based On Symbol.
[**iserver_secdef_strikes_get**](TradingContractsApi.md#iserver_secdef_strikes_get) | **GET** /iserver/secdef/strikes | Get Strikes
[**trsrv_all_conids_get**](TradingContractsApi.md#trsrv_all_conids_get) | **GET** /trsrv/all-conids | All Conids By Exchange
[**trsrv_futures_get**](TradingContractsApi.md#trsrv_futures_get) | **GET** /trsrv/futures | Future  Security Definition By Symbol
[**trsrv_secdef_get**](TradingContractsApi.md#trsrv_secdef_get) | **GET** /trsrv/secdef | Search The Security Definition By Contract ID
[**trsrv_secdef_schedule_get**](TradingContractsApi.md#trsrv_secdef_schedule_get) | **GET** /trsrv/secdef/schedule | Trading Schedule By Symbol
[**trsrv_stocks_get**](TradingContractsApi.md#trsrv_stocks_get) | **GET** /trsrv/stocks | Stock Security Definition By Symbol

## iserver_contract_conid_algos_get

> models::AlgosResponse iserver_contract_conid_algos_get(conid, algos, add_description, add_params)
Search Algo Params By Contract ID

Returns supported IB Algos for contract. A pre-flight request must be submitted before retrieving information

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** |  | [required] |
**algos** | Option<**String**> |  |  |
**add_description** | Option<**String**> |  |  |[default to 0]
**add_params** | Option<**String**> |  |  |[default to 0]

### Return type

[**models::AlgosResponse**](algosResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_contract_conid_info_and_rules_get

> models::IserverContractConidInfoAndRulesGet200Response iserver_contract_conid_info_and_rules_get(conid)
Contract Information And Rules By Contract ID

Requests full contract details for the given conid.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** |  | [required] |

### Return type

[**models::IserverContractConidInfoAndRulesGet200Response**](_iserver_contract__conid__info_and_rules_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_contract_conid_info_get

> models::ContractInfo iserver_contract_conid_info_get(conid)
Contract Information By Contract ID

Requests full contract details for the given conid.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** |  | [required] |

### Return type

[**models::ContractInfo**](contractInfo.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_contract_rules_post

> models::ContractRules iserver_contract_rules_post(iserver_contract_rules_post_request)
Search Contract Rules

Returns trading related rules for a specific contract and side.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_contract_rules_post_request** | [**IserverContractRulesPostRequest**](IserverContractRulesPostRequest.md) |  | [required] |

### Return type

[**models::ContractRules**](contractRules.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_currency_pairs_get

> std::collections::HashMap<String, Vec<models::CurrencyPairsValueInner>> iserver_currency_pairs_get(currency)

Obtains available currency pairs corresponding to the given target currency.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**currency** | **String** |  | [required] |

### Return type

[**std::collections::HashMap<String, Vec<models::CurrencyPairsValueInner>>**](Vec.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_exchangerate_get

> models::IserverExchangerateGet200Response iserver_exchangerate_get(target, source)
Currency Exchange Rate

Obtains the exchange rates of the currency pair.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**target** | **String** |  | [required] |
**source** | **String** |  | [required] |

### Return type

[**models::IserverExchangerateGet200Response**](_iserver_exchangerate_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_secdef_bond_filters_get

> models::BondFiltersResponse iserver_secdef_bond_filters_get(symbol, issue_id)
Search Bond Filter Information

Request a list of filters relating to a given Bond issuerID. The issuerId is retrieved from /iserver/secdef/search and can be used in /iserver/secdef/info?issuerId={issuerId} for retrieving conIds.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**symbol** | **String** |  | [required] |
**issue_id** | **String** |  | [required] |

### Return type

[**models::BondFiltersResponse**](bondFiltersResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_secdef_info_get

> models::SecDefInfoResponse iserver_secdef_info_get(conid, sectype, month, exchange, strike, right, issuer_id, filters)
SecDef Info

SecDef info

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | Option<**String**> |  |  |
**sectype** | Option<[**serde_json::Value**](.md)> |  |  |
**month** | Option<[**serde_json::Value**](.md)> |  |  |
**exchange** | Option<[**serde_json::Value**](.md)> |  |  |
**strike** | Option<[**serde_json::Value**](.md)> |  |  |
**right** | Option<**String**> |  |  |
**issuer_id** | Option<**String**> |  |  |
**filters** | Option<[**serde_json::Value**](.md)> |  |  |

### Return type

[**models::SecDefInfoResponse**](secDefInfoResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_secdef_search_get

> Vec<models::SecdefSearchResponseInner> iserver_secdef_search_get(symbol, sec_type, name, more, fund, fund_family_conid_ex, pattern, referrer)
Returns A List Of Contracts Based On Symbol.

Returns a list of contracts based on the search symbol provided as a query param.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**symbol** | Option<**String**> |  |  |
**sec_type** | Option<**String**> | Available underlying security types:   * `STK` - Represents an underlying as a Stock security type.   * `IND` - Represents an underlying as an Index security type.   * `BOND` - Represents an underlying as a Bond security type.  |  |[default to STK]
**name** | Option<**bool**> |  |  |
**more** | Option<**bool**> |  |  |
**fund** | Option<**bool**> |  |  |
**fund_family_conid_ex** | Option<**String**> |  |  |
**pattern** | Option<**bool**> |  |  |
**referrer** | Option<**String**> |  |  |

### Return type

[**Vec<models::SecdefSearchResponseInner>**](secdefSearchResponse_inner.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_secdef_search_post

> Vec<models::SecdefSearchResponseInner> iserver_secdef_search_post(iserver_secdef_search_post_request)
Returns A List Of Contracts Based On Symbol.

Returns a list of contracts based on the search symbol provided as a query param.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_secdef_search_post_request** | [**IserverSecdefSearchPostRequest**](IserverSecdefSearchPostRequest.md) |  | [required] |

### Return type

[**Vec<models::SecdefSearchResponseInner>**](secdefSearchResponse_inner.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_secdef_strikes_get

> models::IserverSecdefStrikesGet200Response iserver_secdef_strikes_get(conid, sectype, month, exchange)
Get Strikes

strikes

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** |  | [required] |
**sectype** | **String** |  | [required] |
**month** | **String** |  | [required] |
**exchange** | Option<**String**> |  |  |[default to SMART]

### Return type

[**models::IserverSecdefStrikesGet200Response**](_iserver_secdef_strikes_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## trsrv_all_conids_get

> Vec<models::TrsrvAllConidsGet200ResponseInner> trsrv_all_conids_get(exchange, asset_class)
All Conids By Exchange

Send out a request to retrieve all contracts made available on a requested exchange. This returns all contracts that are tradable on the exchange, even those that are not using the exchange as their primary listing.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**exchange** | **String** |  | [required] |
**asset_class** | Option<[**serde_json::Value**](.md)> |  |  |[default to STK]

### Return type

[**Vec<models::TrsrvAllConidsGet200ResponseInner>**](_trsrv_all_conids_get_200_response_inner.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## trsrv_futures_get

> models::Features trsrv_futures_get(symbols, exchange)
Future  Security Definition By Symbol

Returns a list of non-expired future contracts for given symbol(s)

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**symbols** | **String** |  | [required] |
**exchange** | Option<[**serde_json::Value**](.md)> |  |  |

### Return type

[**models::Features**](features.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## trsrv_secdef_get

> models::TrsrvSecDefResponse trsrv_secdef_get(conids, UNKNOWN_PARAMETER_NAME, UNKNOWN_PARAMETER_NAME2)
Search The Security Definition By Contract ID

Returns a list of security definitions for the given conids.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conids** | **String** |  | [required] |
**UNKNOWN_PARAMETER_NAME** | Option<[****](.md)> |  |  |
**UNKNOWN_PARAMETER_NAME2** | Option<[****](.md)> |  |  |

### Return type

[**models::TrsrvSecDefResponse**](trsrvSecDefResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## trsrv_secdef_schedule_get

> Vec<models::TradingScheduleInner> trsrv_secdef_schedule_get(asset_class, symbol, exchange, exchange_filter)
Trading Schedule By Symbol

Returns the trading schedule up to a month for the requested contract.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**asset_class** | **String** |  | [required] |
**symbol** | **String** |  | [required] |
**exchange** | Option<**String**> |  |  |
**exchange_filter** | Option<**String**> |  |  |

### Return type

[**Vec<models::TradingScheduleInner>**](tradingSchedule_inner.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## trsrv_stocks_get

> std::collections::HashMap<String, Vec<models::StocksValueInner>> trsrv_stocks_get(symbols)
Stock Security Definition By Symbol

Returns an object contains all stock contracts for given symbol(s)

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**symbols** | **String** |  | [required] |

### Return type

[**std::collections::HashMap<String, Vec<models::StocksValueInner>>**](Vec.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
