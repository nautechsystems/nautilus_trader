# \TradingOAuth10aApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**oauth_access_token_post**](TradingOAuth10aApi.md#oauth_access_token_post) | **POST** /oauth/access_token | Request An Access Token For The IB Username That Has Granted Authorization To The Consumer.
[**oauth_live_session_token_post**](TradingOAuth10aApi.md#oauth_live_session_token_post) | **POST** /oauth/live_session_token | Generate A Live Session Token Shared Secret And Gain Access To Web API.
[**oauth_request_token_post**](TradingOAuth10aApi.md#oauth_request_token_post) | **POST** /oauth/request_token | Request A Temporary Token As A Third Party To Begin The OAuth 1.0a Authorization Workflow.

## oauth_access_token_post

> models::OauthAccessTokenPost200Response oauth_access_token_post(authorization)
Request An Access Token For The IB Username That Has Granted Authorization To The Consumer.

Request an access token for the IB username that has granted authorization to the consumer.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | Option<**String**> | OAuth 1.0a authorization request header for request to /access_token endpoint. |  |

### Return type

[**models::OauthAccessTokenPost200Response**](_oauth_access_token_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## oauth_live_session_token_post

> models::OauthLiveSessionTokenPost200Response oauth_live_session_token_post(authorization)
Generate A Live Session Token Shared Secret And Gain Access To Web API.

Generate a Live Session Token shared secret and gain access to Web API.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | Option<**String**> | OAuth 1.0a authorization request header for request to /live_session_token endpoint. |  |

### Return type

[**models::OauthLiveSessionTokenPost200Response**](_oauth_live_session_token_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## oauth_request_token_post

> models::OauthRequestTokenPost200Response oauth_request_token_post(authorization)
Request A Temporary Token As A Third Party To Begin The OAuth 1.0a Authorization Workflow.

Request a temporary token as a third party to begin the OAuth 1.0a authorization workflow.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | Option<**String**> | OAuth 1.0a authorization request header for request to /request_token endpoint. |  |

### Return type

[**models::OauthRequestTokenPost200Response**](_oauth_request_token_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
