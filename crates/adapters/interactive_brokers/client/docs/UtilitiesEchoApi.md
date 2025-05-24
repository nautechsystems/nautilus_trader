# \UtilitiesEchoApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**gw_api_v1_echo_https_get**](UtilitiesEchoApi.md#gw_api_v1_echo_https_get) | **GET** /gw/api/v1/echo/https | Echo A Request With HTTPS Security Policy Back After Validation.
[**gw_api_v1_echo_signed_jwt_post**](UtilitiesEchoApi.md#gw_api_v1_echo_signed_jwt_post) | **POST** /gw/api/v1/echo/signed-jwt | Echo A Request With Signed JWT Security Policy Back After Validation.

## gw_api_v1_echo_https_get

> models::EchoResponse gw_api_v1_echo_https_get()
Echo A Request With HTTPS Security Policy Back After Validation.

<br>**Scope**: `echo.read`<br>**Security Policy**: `HTTPS`

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::EchoResponse**](EchoResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_echo_signed_jwt_post

> models::EchoResponse gw_api_v1_echo_signed_jwt_post(signed_jwt_echo_request)
Echo A Request With Signed JWT Security Policy Back After Validation.

<br>**Scope**: `echo.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**signed_jwt_echo_request** | [**SignedJwtEchoRequest**](SignedJwtEchoRequest.md) | Create a Signed JWT echo request. | [required] |

### Return type

[**models::EchoResponse**](EchoResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/jwt
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
