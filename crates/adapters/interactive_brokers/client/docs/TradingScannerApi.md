# \TradingScannerApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**hmds_scanner_params_get**](TradingScannerApi.md#hmds_scanner_params_get) | **GET** /hmds/scanner/params | HMDS Scanner Parameters
[**hmds_scanner_run_post**](TradingScannerApi.md#hmds_scanner_run_post) | **POST** /hmds/scanner/run | HMDS Market Scanner
[**iserver_scanner_params_get**](TradingScannerApi.md#iserver_scanner_params_get) | **GET** /iserver/scanner/params | Iserver Scanner Parameters
[**iserver_scanner_run_post**](TradingScannerApi.md#iserver_scanner_run_post) | **POST** /iserver/scanner/run | Iserver Market Scanner

## hmds_scanner_params_get

> models::HmdsScannerParams hmds_scanner_params_get()
HMDS Scanner Parameters

Query the parameter list for the HMDS market scanner.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::HmdsScannerParams**](hmdsScannerParams.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## hmds_scanner_run_post

> models::HmdsScannerRunPost200Response hmds_scanner_run_post(hmds_scanner_run_request)
HMDS Market Scanner

Request a market scanner from our HMDS service. Can return a maximum of 250 contracts.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**hmds_scanner_run_request** | [**HmdsScannerRunRequest**](HmdsScannerRunRequest.md) |  | [required] |

### Return type

[**models::HmdsScannerRunPost200Response**](_hmds_scanner_run_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8, text/html

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_scanner_params_get

> models::IserverScannerParams iserver_scanner_params_get()
Iserver Scanner Parameters

Returns an xml file containing all available parameters to be sent for the Iserver scanner request.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::IserverScannerParams**](iserverScannerParams.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_scanner_run_post

> models::IserverScannerRunResponse iserver_scanner_run_post(iserver_scanner_run_request)
Iserver Market Scanner

Searches for contracts according to the filters specified in /iserver/scanner/params endpoint.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_scanner_run_request** | [**IserverScannerRunRequest**](IserverScannerRunRequest.md) |  | [required] |

### Return type

[**models::IserverScannerRunResponse**](iserverScannerRunResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
