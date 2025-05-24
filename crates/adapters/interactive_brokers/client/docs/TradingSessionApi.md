# \TradingSessionApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**iserver_auth_ssodh_init_post**](TradingSessionApi.md#iserver_auth_ssodh_init_post) | **POST** /iserver/auth/ssodh/init | Initialize Brokerage Session.
[**iserver_auth_status_post**](TradingSessionApi.md#iserver_auth_status_post) | **POST** /iserver/auth/status | Brokerage Session Auth Status
[**iserver_reauthenticate_get**](TradingSessionApi.md#iserver_reauthenticate_get) | **GET** /iserver/reauthenticate | Re-authenticate The Brokerage Session
[**logout_post**](TradingSessionApi.md#logout_post) | **POST** /logout | Logout Of The Current Session.
[**sso_validate_get**](TradingSessionApi.md#sso_validate_get) | **GET** /sso/validate | Validate SSO
[**tickle_post**](TradingSessionApi.md#tickle_post) | **POST** /tickle | Server Ping.

## iserver_auth_ssodh_init_post

> models::BrokerageSessionStatus iserver_auth_ssodh_init_post(brokerage_session_init_request)
Initialize Brokerage Session.

After retrieving the access token and subsequent Live Session Token, customers can initialize their brokerage session with the ssodh/init endpoint.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**brokerage_session_init_request** | [**BrokerageSessionInitRequest**](BrokerageSessionInitRequest.md) |  | [required] |

### Return type

[**models::BrokerageSessionStatus**](brokerageSessionStatus.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_auth_status_post

> models::BrokerageSessionStatus iserver_auth_status_post()
Brokerage Session Auth Status

Current Authentication status to the Brokerage system. Market Data and Trading is not possible if not authenticated.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::BrokerageSessionStatus**](brokerageSessionStatus.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_reauthenticate_get

> models::IserverReauthenticateGet200Response iserver_reauthenticate_get()
Re-authenticate The Brokerage Session

When using the CP Gateway, this endpoint provides a way to reauthenticate to the Brokerage system as long as there is a valid brokerage session.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::IserverReauthenticateGet200Response**](_iserver_reauthenticate_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## logout_post

> models::LogoutPost200Response logout_post()
Logout Of The Current Session.

Logs the user out of the gateway session. Any further activity requires re-authentication.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::LogoutPost200Response**](_logout_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## sso_validate_get

> models::SsoValidateResponse sso_validate_get()
Validate SSO

Validates the current session for the SSO user.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::SsoValidateResponse**](ssoValidateResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## tickle_post

> models::TickleResponse tickle_post()
Server Ping.

If the gateway has not received any requests for several minutes an open session will automatically timeout. The tickle endpoint pings the server to prevent the session from ending. It is expected to call this endpoint approximately every 60 seconds to maintain the connection to the brokerage session.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::TickleResponse**](tickleResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
