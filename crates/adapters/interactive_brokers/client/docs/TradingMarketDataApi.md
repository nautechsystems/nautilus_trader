# \TradingMarketDataApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**hmds_history_get**](TradingMarketDataApi.md#hmds_history_get) | **GET** /hmds/history | Request Historical Data For An Instrument In The Form Of OHLC Bars.
[**iserver_marketdata_history_get**](TradingMarketDataApi.md#iserver_marketdata_history_get) | **GET** /iserver/marketdata/history | Request Historical Data For An Instrument In The Form Of OHLC Bars.
[**iserver_marketdata_snapshot_get**](TradingMarketDataApi.md#iserver_marketdata_snapshot_get) | **GET** /iserver/marketdata/snapshot | Live Market Data Snapshot
[**iserver_marketdata_unsubscribe_post**](TradingMarketDataApi.md#iserver_marketdata_unsubscribe_post) | **POST** /iserver/marketdata/unsubscribe | Instruct IServer To Close Its Backend Stream For The Instrument.
[**iserver_marketdata_unsubscribeall_get**](TradingMarketDataApi.md#iserver_marketdata_unsubscribeall_get) | **GET** /iserver/marketdata/unsubscribeall | Instruct IServer To Close All Of Its Open Backend Data Streams For All Instruments.
[**md_regsnapshot_get**](TradingMarketDataApi.md#md_regsnapshot_get) | **GET** /md/regsnapshot | Request A Regulatory Snapshot For An Instrument.

## hmds_history_get

> models::HmdsHistoryResponse hmds_history_get(conid, period, bar, bar_type, start_time, direction, outside_rth)
Request Historical Data For An Instrument In The Form Of OHLC Bars.

Request historical data for an instrument in the form of OHLC bars.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** | IB contract ID of the requested instrument. | [required] |
**period** | **String** | A time duration away from startTime, as governed by the direction parameter, to be divided into bars of the specified width. | [required] |
**bar** | **String** | The width of the bars into which the interval determined by period and startTime will be divided. It is not required that bar evenly divide period; partial bars can be returned. | [required] |
**bar_type** | Option<**String**> | The requested historical data type. If omitted, Last Trade data is queried. |  |
**start_time** | Option<**String**> | A fixed UTC date-time reference point for the historical data request, from which the specified period extends, as governed by the direction parameter. Format is YYYYMMDD-hh:mm:ss. If omitted, the current time is used, and direction must be omitted or 1. |  |
**direction** | Option<**String**> | The requested period's direction in time away from the startTime. -1 queries bars from startTime forward into the future for the span of the requested period, 1 queries bars from startTime backward into the past for the span of the request period. Default behavior is 1, from startTime backward. |  |
**outside_rth** | Option<**bool**> | Indicates whether data outside of regular trading hours should be included in response. |  |

### Return type

[**models::HmdsHistoryResponse**](hmdsHistoryResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_marketdata_history_get

> models::IserverHistoryResponse iserver_marketdata_history_get(conid, period, bar, exchange, start_time, outside_rth)
Request Historical Data For An Instrument In The Form Of OHLC Bars.

Request historical data for an instrument in the form of OHLC bars.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** | IB contract ID of the requested instrument. | [required] |
**period** | **String** | A time duration away from startTime into the future to be divided into bars of the specified width. | [required] |
**bar** | **String** | The width of the bars into which the interval determined by period and startTime will be divided. It is not required that bar evenly divide period; partial bars can be returned. | [required] |
**exchange** | Option<**String**> | Exchange (or SMART) from which data is requested. |  |
**start_time** | Option<**String**> | A fixed UTC date-time reference point for the historical data request, from which the specified period extends. Format is YYYYMMDD-hh:mm:ss. If omitted, the current time is used, and direction must be omitted or 1. |  |
**outside_rth** | Option<**bool**> | Indicates whether data outside of regular trading hours should be included in response. |  |

### Return type

[**models::IserverHistoryResponse**](iserverHistoryResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_marketdata_snapshot_get

> models::FyiVt iserver_marketdata_snapshot_get(conids, fields)
Live Market Data Snapshot

Get Market Data for the given conid(s). A pre-flight request must be made prior to ever receiving data. For some fields, it may take more than a few moments to receive information. See response fields for a list of available fields that can be request via fields argument. The endpoint /iserver/accounts must be called prior to /iserver/marketdata/snapshot. For derivative contracts the endpoint /iserver/secdef/search must be called first.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conids** | **String** |  | [required] |
**fields** | Option<[**MdFields**](.md)> |  |  |

### Return type

[**models::FyiVt**](fyiVT.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_marketdata_unsubscribe_post

> models::IserverMarketdataUnsubscribePost200Response iserver_marketdata_unsubscribe_post(iserver_marketdata_unsubscribe_post_request)
Instruct IServer To Close Its Backend Stream For The Instrument.

Instruct IServer to close its backend stream for the instrument when real-time snapshots are no longer needed.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_marketdata_unsubscribe_post_request** | [**IserverMarketdataUnsubscribePostRequest**](IserverMarketdataUnsubscribePostRequest.md) |  | [required] |

### Return type

[**models::IserverMarketdataUnsubscribePost200Response**](_iserver_marketdata_unsubscribe_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_marketdata_unsubscribeall_get

> models::IserverMarketdataUnsubscribeallGet200Response iserver_marketdata_unsubscribeall_get()
Instruct IServer To Close All Of Its Open Backend Data Streams For All Instruments.

Instruct IServer to close all of its open backend data streams for all instruments.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::IserverMarketdataUnsubscribeallGet200Response**](_iserver_marketdata_unsubscribeall_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## md_regsnapshot_get

> models::RegsnapshotData md_regsnapshot_get(conid)
Request A Regulatory Snapshot For An Instrument.

Request a regulatory snapshot for an instrument.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** | An IB contract ID. | [required] |

### Return type

[**models::RegsnapshotData**](regsnapshotData.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
