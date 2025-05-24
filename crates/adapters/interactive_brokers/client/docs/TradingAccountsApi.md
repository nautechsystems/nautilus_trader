# \TradingAccountsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**acesws_account_id_signatures_and_owners_get**](TradingAccountsApi.md#acesws_account_id_signatures_and_owners_get) | **GET** /acesws/{accountId}/signatures-and-owners | Signatures And Owners
[**iserver_account_account_id_summary_available_funds_get**](TradingAccountsApi.md#iserver_account_account_id_summary_available_funds_get) | **GET** /iserver/account/{accountId}/summary/available_funds | Summary Of Available Funds
[**iserver_account_account_id_summary_balances_get**](TradingAccountsApi.md#iserver_account_account_id_summary_balances_get) | **GET** /iserver/account/{accountId}/summary/balances | Summary Of Account Balances
[**iserver_account_account_id_summary_get**](TradingAccountsApi.md#iserver_account_account_id_summary_get) | **GET** /iserver/account/{accountId}/summary | General Account Summary
[**iserver_account_account_id_summary_margins_get**](TradingAccountsApi.md#iserver_account_account_id_summary_margins_get) | **GET** /iserver/account/{accountId}/summary/margins | Summary Of Account Margin
[**iserver_account_account_id_summary_market_value_get**](TradingAccountsApi.md#iserver_account_account_id_summary_market_value_get) | **GET** /iserver/account/{accountId}/summary/market_value | Summary Of Account's Market Value
[**iserver_account_pnl_partitioned_get**](TradingAccountsApi.md#iserver_account_pnl_partitioned_get) | **GET** /iserver/account/pnl/partitioned | Account Profit And Loss
[**iserver_account_post**](TradingAccountsApi.md#iserver_account_post) | **POST** /iserver/account | Switch Account
[**iserver_account_search_search_pattern_get**](TradingAccountsApi.md#iserver_account_search_search_pattern_get) | **GET** /iserver/account/search/{searchPattern} | Search Dynamic Account
[**iserver_accounts_get**](TradingAccountsApi.md#iserver_accounts_get) | **GET** /iserver/accounts | Receive Brokerage Accounts For The Current User.
[**iserver_dynaccount_post**](TradingAccountsApi.md#iserver_dynaccount_post) | **POST** /iserver/dynaccount | Set Dynamic Account

## acesws_account_id_signatures_and_owners_get

> models::SignatureAndOwners acesws_account_id_signatures_and_owners_get(account_id)
Signatures And Owners

Receive a list of all applicant names on the account and for which account and entity is represented.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::SignatureAndOwners**](signatureAndOwners.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_summary_available_funds_get

> models::AvailableFundsResponse iserver_account_account_id_summary_available_funds_get(account_id)
Summary Of Available Funds

Provides a summary specific for avilable funds giving more depth than the standard /summary endpoint.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::AvailableFundsResponse**](availableFundsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_summary_balances_get

> models::SummaryOfAccountBalancesResponse iserver_account_account_id_summary_balances_get(account_id)
Summary Of Account Balances

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::SummaryOfAccountBalancesResponse**](summaryOfAccountBalancesResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_summary_get

> models::AccountSummaryResponse iserver_account_account_id_summary_get(account_id)
General Account Summary

Provides a general overview of the account details such as balance values.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::AccountSummaryResponse**](accountSummaryResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_summary_margins_get

> models::SummaryOfAccountMarginResponse iserver_account_account_id_summary_margins_get(account_id)
Summary Of Account Margin

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::SummaryOfAccountMarginResponse**](summaryOfAccountMarginResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_account_id_summary_market_value_get

> models::SummaryMarketValueResponse iserver_account_account_id_summary_market_value_get(account_id)
Summary Of Account's Market Value

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::SummaryMarketValueResponse**](summaryMarketValueResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_pnl_partitioned_get

> models::PnlPartitionedResponse iserver_account_pnl_partitioned_get()
Account Profit And Loss

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::PnlPartitionedResponse**](pnlPartitionedResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_post

> models::SetAccountResponse iserver_account_post(iserver_account_post_request)
Switch Account

Switch the active account for how you request data. Only available for financial advisors and multi-account structures.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_account_post_request** | [**IserverAccountPostRequest**](IserverAccountPostRequest.md) |  | [required] |

### Return type

[**models::SetAccountResponse**](setAccountResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_account_search_search_pattern_get

> models::DynAccountSearchResponse iserver_account_search_search_pattern_get(search_pattern)
Search Dynamic Account

Returns a list of accounts matching a query pattern set in the request. Broker accounts configured with the DYNACCT property will not receive account information at login. Instead, they must dynamically query then set their account number. Customers without the DYNACCT property will receive a 503 error.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**search_pattern** | **String** |  | [required] |

### Return type

[**models::DynAccountSearchResponse**](dynAccountSearchResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_accounts_get

> models::UserAccountsResponse iserver_accounts_get()
Receive Brokerage Accounts For The Current User.

Returns a list of accounts the user has trading access to, their respective aliases and the currently selected account. Note this endpoint must be called before modifying an order or querying open orders.

### Parameters

This endpoint does not need any parameter.

### Return type

[**models::UserAccountsResponse**](userAccountsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_dynaccount_post

> models::SetAccountResponse iserver_dynaccount_post(iserver_dynaccount_post_request)
Set Dynamic Account

Set the active dynamic account.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_dynaccount_post_request** | [**IserverDynaccountPostRequest**](IserverDynaccountPostRequest.md) |  | [required] |

### Return type

[**models::SetAccountResponse**](setAccountResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
