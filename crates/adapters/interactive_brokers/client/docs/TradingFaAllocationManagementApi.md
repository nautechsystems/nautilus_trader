# \TradingFaAllocationManagementApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**iserver_account_allocation_accounts_get**](TradingFaAllocationManagementApi.md#iserver_account_allocation_accounts_get) | **GET** /iserver/account/allocation/accounts | Allocatable Sub-Accounts
[**iserver_account_allocation_group_delete_post**](TradingFaAllocationManagementApi.md#iserver_account_allocation_group_delete_post) | **POST** /iserver/account/allocation/group/delete | Remove Allocation Group
[**iserver_account_allocation_group_get**](TradingFaAllocationManagementApi.md#iserver_account_allocation_group_get) | **GET** /iserver/account/allocation/group | List All Allocation Groups
[**iserver_account_allocation_group_post**](TradingFaAllocationManagementApi.md#iserver_account_allocation_group_post) | **POST** /iserver/account/allocation/group | Add Allocation Group
[**iserver_account_allocation_group_put**](TradingFaAllocationManagementApi.md#iserver_account_allocation_group_put) | **PUT** /iserver/account/allocation/group | Modify Allocation Group
[**iserver_account_allocation_group_single_post**](TradingFaAllocationManagementApi.md#iserver_account_allocation_group_single_post) | **POST** /iserver/account/allocation/group/single | Retrieve Single Allocation Group
[**iserver_account_allocation_presets_get**](TradingFaAllocationManagementApi.md#iserver_account_allocation_presets_get) | **GET** /iserver/account/allocation/presets | Retrieve Allocation Presets
[**iserver_account_allocation_presets_post**](TradingFaAllocationManagementApi.md#iserver_account_allocation_presets_post) | **POST** /iserver/account/allocation/presets | Set The Allocation Presets

## iserver_account_allocation_accounts_get

> models::SubAccounts iserver_account_allocation_accounts_get()
Allocatable Sub-Accounts

Retrieves a list of all sub-accounts and returns their net liquidity and available equity for advisors to make decisions on what accounts should be allocated and how.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::SubAccounts**](subAccounts.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_allocation_group_delete_post

> models::IserverAccountAllocationGroupPut200Response iserver_account_allocation_group_delete_post(iserver_account_allocation_group_delete_post_request)
Remove Allocation Group

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_account_allocation_group_delete_post_request** | [**IserverAccountAllocationGroupDeletePostRequest**](IserverAccountAllocationGroupDeletePostRequest.md) |  | [required] |

### Return type

[**models::IserverAccountAllocationGroupPut200Response**](_iserver_account_allocation_group_put_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_allocation_group_get

> models::AllocationGroups iserver_account_allocation_group_get()
List All Allocation Groups

Retrieves a list of all of the advisor’s allocation groups. This describes the name of the allocation group, number of subaccounts within the group, and the method in use for the group.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::AllocationGroups**](allocationGroups.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_allocation_group_post

> models::IserverAccountAllocationGroupPut200Response iserver_account_allocation_group_post(iserver_account_allocation_group_post_request)
Add Allocation Group

Add a new allocation group. This group can be used to trade in place of the {accountId} for the /iserver/account/{accountId}/orders endpoint.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_account_allocation_group_post_request** | [**IserverAccountAllocationGroupPostRequest**](IserverAccountAllocationGroupPostRequest.md) |  | [required] |

### Return type

[**models::IserverAccountAllocationGroupPut200Response**](_iserver_account_allocation_group_put_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_allocation_group_put

> models::IserverAccountAllocationGroupPut200Response iserver_account_allocation_group_put(iserver_account_allocation_group_put_request)
Modify Allocation Group

Modify an existing allocation group.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_account_allocation_group_put_request** | [**IserverAccountAllocationGroupPutRequest**](IserverAccountAllocationGroupPutRequest.md) |  | [required] |

### Return type

[**models::IserverAccountAllocationGroupPut200Response**](_iserver_account_allocation_group_put_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_allocation_group_single_post

> models::AllocationGroup iserver_account_allocation_group_single_post(iserver_account_allocation_group_delete_post_request)
Retrieve Single Allocation Group

Retrieves the configuration of a single account group. This describes the name of the allocation group, the specific accounts contained in the group, and the allocation method in use along with any relevant quantities.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_account_allocation_group_delete_post_request** | [**IserverAccountAllocationGroupDeletePostRequest**](IserverAccountAllocationGroupDeletePostRequest.md) |  | [required] |

### Return type

[**models::AllocationGroup**](allocationGroup.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_allocation_presets_get

> models::Presets iserver_account_allocation_presets_get()
Retrieve Allocation Presets

Retrieve the preset behavior for allocation groups for specific events.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::Presets**](presets.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_allocation_presets_post

> models::IserverAccountAllocationPresetsPost200Response iserver_account_allocation_presets_post(presets)
Set The Allocation Presets

Set the preset behavior for new allocation groups for specific events.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**presets** | [**Presets**](Presets.md) |  | [required] |

### Return type

[**models::IserverAccountAllocationPresetsPost200Response**](_iserver_account_allocation_presets_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
