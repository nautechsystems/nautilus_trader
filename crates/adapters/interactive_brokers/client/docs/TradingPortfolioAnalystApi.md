# \TradingPortfolioAnalystApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**pa_allperiods_post**](TradingPortfolioAnalystApi.md#pa_allperiods_post) | **POST** /pa/allperiods | Account Performance (All Time Periods)
[**pa_performance_post**](TradingPortfolioAnalystApi.md#pa_performance_post) | **POST** /pa/performance | Account Performance
[**pa_transactions_post**](TradingPortfolioAnalystApi.md#pa_transactions_post) | **POST** /pa/transactions | Transaction History

## pa_allperiods_post

> models::DetailedContractInformation pa_allperiods_post(pa_allperiods_post_request, param0)
Account Performance (All Time Periods)

Returns the performance (MTM) for the given accounts, if more than one account is passed, the result is consolidated.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**pa_allperiods_post_request** | [**PaAllperiodsPostRequest**](PaAllperiodsPostRequest.md) |  | [required] |
**param0** | Option<**String**> |  |  |

### Return type

[**models::DetailedContractInformation**](detailedContractInformation.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## pa_performance_post

> models::PerformanceResponse pa_performance_post(pa_performance_post_request)
Account Performance

Returns the performance (MTM) for the given accounts, if more than one account is passed, the result is consolidated.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**pa_performance_post_request** | [**PaPerformancePostRequest**](PaPerformancePostRequest.md) |  | [required] |

### Return type

[**models::PerformanceResponse**](performanceResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## pa_transactions_post

> models::TransactionsResponse pa_transactions_post(pa_transactions_post_request)
Transaction History

Transaction history for a given number of conids and accounts. Types of transactions include dividend payments, buy and sell transactions, transfers.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**pa_transactions_post_request** | [**PaTransactionsPostRequest**](PaTransactionsPostRequest.md) |  | [required] |

### Return type

[**models::TransactionsResponse**](transactionsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
