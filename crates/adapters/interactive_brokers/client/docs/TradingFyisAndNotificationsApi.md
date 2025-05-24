# \TradingFyisAndNotificationsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**fyi_deliveryoptions_device_id_delete**](TradingFyisAndNotificationsApi.md#fyi_deliveryoptions_device_id_delete) | **DELETE** /fyi/deliveryoptions/{deviceId} | Delete A Device
[**fyi_deliveryoptions_device_post**](TradingFyisAndNotificationsApi.md#fyi_deliveryoptions_device_post) | **POST** /fyi/deliveryoptions/device | Enable/Disable Device Option
[**fyi_deliveryoptions_email_put**](TradingFyisAndNotificationsApi.md#fyi_deliveryoptions_email_put) | **PUT** /fyi/deliveryoptions/email | Enable/Disable Email Option
[**fyi_deliveryoptions_get**](TradingFyisAndNotificationsApi.md#fyi_deliveryoptions_get) | **GET** /fyi/deliveryoptions | Get Delivery OptionsCopy Location
[**fyi_disclaimer_typecode_get**](TradingFyisAndNotificationsApi.md#fyi_disclaimer_typecode_get) | **GET** /fyi/disclaimer/{typecode} | Get Disclaimer For A Certain Kind Of Fyi
[**fyi_disclaimer_typecode_put**](TradingFyisAndNotificationsApi.md#fyi_disclaimer_typecode_put) | **PUT** /fyi/disclaimer/{typecode} | Mark Disclaimer Read
[**fyi_notifications_get**](TradingFyisAndNotificationsApi.md#fyi_notifications_get) | **GET** /fyi/notifications | Get A List Of Notifications
[**fyi_notifications_notification_id_put**](TradingFyisAndNotificationsApi.md#fyi_notifications_notification_id_put) | **PUT** /fyi/notifications/{notificationID} | Mark Notification Read
[**fyi_settings_get**](TradingFyisAndNotificationsApi.md#fyi_settings_get) | **GET** /fyi/settings | Get Notification Settings
[**fyi_settings_typecode_post**](TradingFyisAndNotificationsApi.md#fyi_settings_typecode_post) | **POST** /fyi/settings/{typecode} | Modify FYI Notifications
[**fyi_unreadnumber_get**](TradingFyisAndNotificationsApi.md#fyi_unreadnumber_get) | **GET** /fyi/unreadnumber | Unread Bulletins

## fyi_deliveryoptions_device_id_delete

> fyi_deliveryoptions_device_id_delete(device_id)
Delete A Device

Delete a specific device from our saved list of notification devices.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**device_id** | **String** |  | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_deliveryoptions_device_post

> models::FyiVt fyi_deliveryoptions_device_post(fyi_enable_device_option)
Enable/Disable Device Option

Choose whether a particular device is enabled or disabled.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**fyi_enable_device_option** | [**FyiEnableDeviceOption**](FyiEnableDeviceOption.md) |  | [required] |

### Return type

[**models::FyiVt**](fyiVT.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_deliveryoptions_email_put

> models::FyiVt fyi_deliveryoptions_email_put(enabled)
Enable/Disable Email Option

Enable or disable your account’s primary email to receive notifications.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**enabled** | [**serde_json::Value**](.md) |  | [required] |

### Return type

[**models::FyiVt**](fyiVT.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_deliveryoptions_get

> models::DeliveryOptions fyi_deliveryoptions_get()
Get Delivery OptionsCopy Location

Options for sending fyis to email and other devices.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::DeliveryOptions**](deliveryOptions.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_disclaimer_typecode_get

> models::DisclaimerInfo fyi_disclaimer_typecode_get(typecode)
Get Disclaimer For A Certain Kind Of Fyi

Receive additional disclaimers based on the specified typecode.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**typecode** | [**Typecodes**](.md) |  | [required] |

### Return type

[**models::DisclaimerInfo**](disclaimerInfo.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_disclaimer_typecode_put

> models::FyiVt fyi_disclaimer_typecode_put(typecode)
Mark Disclaimer Read

Mark a specific disclaimer message as read.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**typecode** | [**Typecodes**](.md) |  | [required] |

### Return type

[**models::FyiVt**](fyiVT.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_notifications_get

> Vec<models::NotificationsInner> fyi_notifications_get(max, include, exclude, id)
Get A List Of Notifications

Get a list of available notifications.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**max** | **String** |  | [required] |
**include** | Option<[**serde_json::Value**](.md)> |  |  |
**exclude** | Option<[**serde_json::Value**](.md)> |  |  |
**id** | Option<[**serde_json::Value**](.md)> |  |  |

### Return type

[**Vec<models::NotificationsInner>**](notifications_inner.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_notifications_notification_id_put

> models::NotificationReadAcknowledge fyi_notifications_notification_id_put(notification_id)
Mark Notification Read

Mark a particular notification message as read or unread.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**notification_id** | **String** |  | [required] |

### Return type

[**models::NotificationReadAcknowledge**](notificationReadAcknowledge.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_settings_get

> Vec<models::FyiSettingsInner> fyi_settings_get()
Get Notification Settings

Return the current choices of subscriptions for notifications.

### Parameters

This endpoint does not need any parameter.

### Return type

[**Vec<models::FyiSettingsInner>**](fyiSettings_inner.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_settings_typecode_post

> models::FyiVt fyi_settings_typecode_post(typecode, fyi_settings_typecode_post_request)
Modify FYI Notifications

Enable or disable group of notifications by the specific typecode.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**typecode** | [**Typecodes**](.md) |  | [required] |
**fyi_settings_typecode_post_request** | [**FyiSettingsTypecodePostRequest**](FyiSettingsTypecodePostRequest.md) |  | [required] |

### Return type

[**models::FyiVt**](fyiVT.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## fyi_unreadnumber_get

> models::FyiUnreadnumberGet200Response fyi_unreadnumber_get()
Unread Bulletins

Returns the total number of unread notifications

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::FyiUnreadnumberGet200Response**](_fyi_unreadnumber_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
