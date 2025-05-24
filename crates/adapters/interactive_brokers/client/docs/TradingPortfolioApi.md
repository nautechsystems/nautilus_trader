# \TradingPortfolioApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**portfolio_account_id_allocation_get**](TradingPortfolioApi.md#portfolio_account_id_allocation_get) | **GET** /portfolio/{accountId}/allocation | Get An Account's Allocations By Asset Class, Sector Group, And Sector.
[**portfolio_account_id_ledger_get**](TradingPortfolioApi.md#portfolio_account_id_ledger_get) | **GET** /portfolio/{accountId}/ledger | Get Ledger Data For The Given Account.
[**portfolio_account_id_meta_get**](TradingPortfolioApi.md#portfolio_account_id_meta_get) | **GET** /portfolio/{accountId}/meta | Get An Account's Attributes.
[**portfolio_account_id_positions_invalidate_post**](TradingPortfolioApi.md#portfolio_account_id_positions_invalidate_post) | **POST** /portfolio/{accountId}/positions/invalidate | Instructs IB To Discard Cached Portfolio Positions For A Given Account.
[**portfolio_account_id_positions_page_id_get**](TradingPortfolioApi.md#portfolio_account_id_positions_page_id_get) | **GET** /portfolio/{accountId}/positions/{pageId} | Get All Positions In An Account.
[**portfolio_account_id_summary_get**](TradingPortfolioApi.md#portfolio_account_id_summary_get) | **GET** /portfolio/{accountId}/summary | Portfolio Account Summary
[**portfolio_accountid_position_conid_get**](TradingPortfolioApi.md#portfolio_accountid_position_conid_get) | **GET** /portfolio/{accountid}/position/{conid} | Get Position For A Given Instrument In A Single Account.
[**portfolio_accounts_get**](TradingPortfolioApi.md#portfolio_accounts_get) | **GET** /portfolio/accounts | Accounts
[**portfolio_positions_conid_get**](TradingPortfolioApi.md#portfolio_positions_conid_get) | **GET** /portfolio/positions/{conid} | Get Positions In Accounts For A Given Instrument
[**portfolio_subaccounts_get**](TradingPortfolioApi.md#portfolio_subaccounts_get) | **GET** /portfolio/subaccounts | Get Attributes Of Subaccounts In Account Structure

## portfolio_account_id_allocation_get

> models::PortfolioAllocations portfolio_account_id_allocation_get(account_id, model)
Get An Account's Allocations By Asset Class, Sector Group, And Sector.

Get an account's allocations by asset class, sector group, and sector.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**model** | Option<**String**> |  |  |

### Return type

[**models::PortfolioAllocations**](portfolioAllocations.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_account_id_ledger_get

> std::collections::HashMap<String, models::LedgerValue> portfolio_account_id_ledger_get(account_id)
Get Ledger Data For The Given Account.

Get the given account's ledger data detailing its balances by currency.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**std::collections::HashMap<String, models::LedgerValue>**](ledger_value.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_account_id_meta_get

> models::AccountAttributes portfolio_account_id_meta_get(account_id)
Get An Account's Attributes.

Get a single account's attributes and capabilities.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::AccountAttributes**](accountAttributes.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_account_id_positions_invalidate_post

> models::PortfolioAccountIdPositionsInvalidatePost200Response portfolio_account_id_positions_invalidate_post(account_id)
Instructs IB To Discard Cached Portfolio Positions For A Given Account.

Instructs IB to discard cached portfolio positions for a given account, so that the next request for positions delivers freshly obtained data.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::PortfolioAccountIdPositionsInvalidatePost200Response**](_portfolio__accountId__positions_invalidate_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_account_id_positions_page_id_get

> Vec<models::IndividualPosition> portfolio_account_id_positions_page_id_get(account_id, UNKNOWN_PARAMETER_NAME, UNKNOWN_PARAMETER_NAME2, UNKNOWN_PARAMETER_NAME3, page_id, wait_for_sec_def)
Get All Positions In An Account.

Get all positions in an account.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**UNKNOWN_PARAMETER_NAME** | [****](.md) | Name of model | [required] |
**UNKNOWN_PARAMETER_NAME2** | [****](.md) | sorting of result positions by specified field. Defaulted to \"name\" field. | [required] |
**UNKNOWN_PARAMETER_NAME3** | [****](.md) | Sorting direction. Possible values \"a\" - ascending, \"d\" - descending. Defaulted to \"a\" | [required] |
**page_id** | Option<**String**> |  |  |
**wait_for_sec_def** | Option<**bool**> |  |  |

### Return type

[**Vec<models::IndividualPosition>**](individualPosition.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_account_id_summary_get

> models::PortfolioSummary portfolio_account_id_summary_get(account_id)
Portfolio Account Summary

Portfolio account summary

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::PortfolioSummary**](portfolioSummary.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_accountid_position_conid_get

> Vec<models::IndividualPosition> portfolio_accountid_position_conid_get(account_id, conid)
Get Position For A Given Instrument In A Single Account.

Get position for a given instrument in a single account. WaitSecDef attribute is always defaulted to false. It is possible to get position without security definition.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**conid** | **String** |  | [required] |

### Return type

[**Vec<models::IndividualPosition>**](individualPosition.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_accounts_get

> Vec<models::AccountAttributes> portfolio_accounts_get()
Accounts

return accounts

### Parameters

This endpoint does not need any parameter.

### Return type

[**Vec<models::AccountAttributes>**](accountAttributes.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_positions_conid_get

> std::collections::HashMap<String, models::IndividualPosition> portfolio_positions_conid_get(conid)
Get Positions In Accounts For A Given Instrument

Get positions in accounts for a given instrument (no secDef await control)

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**conid** | **String** |  | [required] |

### Return type

[**std::collections::HashMap<String, models::IndividualPosition>**](individualPosition.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## portfolio_subaccounts_get

> Vec<models::AccountAttributes> portfolio_subaccounts_get()
Get Attributes Of Subaccounts In Account Structure

Retrieve attributes of the subaccounts in the account structure.

### Parameters

This endpoint does not need any parameter.

### Return type

[**Vec<models::AccountAttributes>**](accountAttributes.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
