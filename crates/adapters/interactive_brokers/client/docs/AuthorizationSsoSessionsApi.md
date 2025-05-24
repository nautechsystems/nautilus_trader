# \AuthorizationSsoSessionsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**gw_api_v1_sso_browser_sessions_post**](AuthorizationSsoSessionsApi.md#gw_api_v1_sso_browser_sessions_post) | **POST** /gw/api/v1/sso-browser-sessions | Create SSO Browser Session.
[**gw_api_v1_sso_sessions_post**](AuthorizationSsoSessionsApi.md#gw_api_v1_sso_sessions_post) | **POST** /gw/api/v1/sso-sessions | Create A New SSO Session On Behalf Of An End-user.

## gw_api_v1_sso_browser_sessions_post

> models::CreateBrowserSessionResponse gw_api_v1_sso_browser_sessions_post(authorization, create_browser_session_request)
Create SSO Browser Session.

<br>**Scope**: `sso-browser-sessions.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | **String** | Specifies the authorization header value (e.g., Bearer eyJ0eXAiOiJKV1...). | [required] |
**create_browser_session_request** | [**CreateBrowserSessionRequest**](CreateBrowserSessionRequest.md) | Create browser session on behalf of end-user. | [required] |

### Return type

[**models::CreateBrowserSessionResponse**](CreateBrowserSessionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/jwt
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_sso_sessions_post

> models::CreateSessionResponse gw_api_v1_sso_sessions_post(authorization, create_session_request)
Create A New SSO Session On Behalf Of An End-user.

<br>**Scope**: `sso-sessions.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | **String** | Specifies the authorization header value (e.g., Bearer eyJ0eXAiOiJKV1...). | [required] |
**create_session_request** | [**CreateSessionRequest**](CreateSessionRequest.md) | Create session on behalf of end-user. | [required] |

### Return type

[**models::CreateSessionResponse**](CreateSessionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/jwt
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
