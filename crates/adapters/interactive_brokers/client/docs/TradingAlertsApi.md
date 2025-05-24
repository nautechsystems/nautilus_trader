# \TradingAlertsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**iserver_account_account_id_alert_activate_post**](TradingAlertsApi.md#iserver_account_account_id_alert_activate_post) | **POST** /iserver/account/{accountId}/alert/activate | Activate Or Deactivate An Alert
[**iserver_account_account_id_alert_alert_id_delete**](TradingAlertsApi.md#iserver_account_account_id_alert_alert_id_delete) | **DELETE** /iserver/account/{accountId}/alert/{alertId} | Delete An Alert
[**iserver_account_account_id_alert_post**](TradingAlertsApi.md#iserver_account_account_id_alert_post) | **POST** /iserver/account/{accountId}/alert | Create Or Modify Alert
[**iserver_account_account_id_alerts_get**](TradingAlertsApi.md#iserver_account_account_id_alerts_get) | **GET** /iserver/account/{accountId}/alerts | Get A List Of Available Alerts
[**iserver_account_alert_alert_id_get**](TradingAlertsApi.md#iserver_account_alert_alert_id_get) | **GET** /iserver/account/alert/{alertId} | Get Details Of A Specific Alert
[**iserver_account_mta_get**](TradingAlertsApi.md#iserver_account_mta_get) | **GET** /iserver/account/mta | Get MTA Alert

## iserver_account_account_id_alert_activate_post

> models::AlertActivationResponse iserver_account_account_id_alert_activate_post(account_id, alert_activation_request)
Activate Or Deactivate An Alert

Activate or Deactivate existing alerts created for this account. This does not delete alerts, but disables notifications until reactivated.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**alert_activation_request** | [**AlertActivationRequest**](AlertActivationRequest.md) |  | [required] |

### Return type

[**models::AlertActivationResponse**](alertActivationResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_alert_alert_id_delete

> models::AlertDeletionResponse iserver_account_account_id_alert_alert_id_delete(account_id, alert_id, body)
Delete An Alert

Permanently delete an existing alert. Deleting an MTA alert will reset it to the default state.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**alert_id** | **String** |  | [required] |
**body** | **serde_json::Value** |  | [required] |

### Return type

[**models::AlertDeletionResponse**](alertDeletionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_alert_post

> models::AlertCreationResponse iserver_account_account_id_alert_post(account_id, alert_creation_request)
Create Or Modify Alert

Endpoint used to create a new alert, or modify an existing alert.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**alert_creation_request** | [**AlertCreationRequest**](AlertCreationRequest.md) |  | [required] |

### Return type

[**models::AlertCreationResponse**](alertCreationResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_alerts_get

> Vec<models::Alert> iserver_account_account_id_alerts_get(account_id)
Get A List Of Available Alerts

Retrieve a list of all alerts attached to the provided account.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**Vec<models::Alert>**](alert.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_alert_alert_id_get

> models::AlertDetails iserver_account_alert_alert_id_get(alert_id, r#type)
Get Details Of A Specific Alert

Request details of a specific alert by providing the assigned alertId Id.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**alert_id** | **String** |  | [required] |
**r#type** | **String** |  | [required] |

### Return type

[**models::AlertDetails**](alertDetails.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_mta_get

> models::AlertDetails iserver_account_mta_get()
Get MTA Alert

Retrieve information about your MTA alert. Each login user only has one mobile trading assistant (MTA) alert with it’s own unique tool id that cannot be changed. MTA alerts can not be created or deleted, only modified. When modified a new order Id is generated.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::AlertDetails**](alertDetails.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
