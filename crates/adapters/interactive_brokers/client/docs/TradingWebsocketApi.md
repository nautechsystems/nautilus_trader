# \TradingWebsocketApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**ws_get**](TradingWebsocketApi.md#ws_get) | **GET** /ws | Open Websocket.

## ws_get

> ws_get(connection, upgrade, api, oauth_token)
Open Websocket.

Open websocket.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**connection** | **String** |  | [required] |
**upgrade** | **String** |  | [required] |
**api** | **String** | 32-character Web API session cookie value. | [required] |
**oauth_token** | **String** | 8-character OAuth access token. | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
