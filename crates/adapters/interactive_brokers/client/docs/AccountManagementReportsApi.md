# \AccountManagementReportsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**gw_api_v1_statements_available_get**](AccountManagementReportsApi.md#gw_api_v1_statements_available_get) | **GET** /gw/api/v1/statements/available | Fetch Available Daily, Monthly, And Annual Report Dates For An Account Id
[**gw_api_v1_statements_post**](AccountManagementReportsApi.md#gw_api_v1_statements_post) | **POST** /gw/api/v1/statements | Generates Statements In Supported Formats Based On Request Parameters.
[**gw_api_v1_tax_documents_available_get**](AccountManagementReportsApi.md#gw_api_v1_tax_documents_available_get) | **GET** /gw/api/v1/tax-documents/available | Fetch List Of Available Tax Reports/forms/documents For A Specified Account And Tax Year
[**gw_api_v1_tax_documents_post**](AccountManagementReportsApi.md#gw_api_v1_tax_documents_post) | **POST** /gw/api/v1/tax-documents | Fetch Tax Forms In Supported Formats Based On Request Parameters.

## gw_api_v1_statements_available_get

> models::GetAvailableStmtDatesResponse gw_api_v1_statements_available_get(authorization, account_id)
Fetch Available Daily, Monthly, And Annual Report Dates For An Account Id

<br>**Scope**: `statements.read` OR `reports.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | **String** | Specifies the authorization header value (e.g., Bearer eyJ0eXAiOiJKV1...). | [required] |
**account_id** | **String** | Specifies the account id to retrieve information | [required] |

### Return type

[**models::GetAvailableStmtDatesResponse**](GetAvailableStmtDatesResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_statements_post

> models::GetStatementsResponse gw_api_v1_statements_post(authorization, stmt_request)
Generates Statements In Supported Formats Based On Request Parameters.

<br>**Scope**: `statements.read` OR `statements.write` OR `reports.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | **String** | Specifies the authorization header value (e.g., Bearer eyJ0eXAiOiJKV1...). | [required] |
**stmt_request** | [**StmtRequest**](StmtRequest.md) | Report request object | [required] |

### Return type

[**models::GetStatementsResponse**](GetStatementsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/pdf, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_tax_documents_available_get

> models::GetAvailableTaxFormsResponse gw_api_v1_tax_documents_available_get(authorization, account_id, year)
Fetch List Of Available Tax Reports/forms/documents For A Specified Account And Tax Year

<br>**Scope**: `statements.read` OR `reports.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | **String** | Specifies the authorization header value (e.g., Bearer eyJ0eXAiOiJKV1...). | [required] |
**account_id** | **String** | Specifies the account id to retrieve information | [required] |
**year** | **i32** | Specifies the tax year to retrieve information | [required] |

### Return type

[**models::GetAvailableTaxFormsResponse**](GetAvailableTaxFormsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_tax_documents_post

> models::TaxFormResponse gw_api_v1_tax_documents_post(authorization, tax_form_request)
Fetch Tax Forms In Supported Formats Based On Request Parameters.

<br>**Scope**: `statements.write` OR `reports.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**authorization** | **String** | Specifies the authorization header value (e.g., Bearer eyJ0eXAiOiJKV1...). | [required] |
**tax_form_request** | [**TaxFormRequest**](TaxFormRequest.md) | Tax Form request object | [required] |

### Return type

[**models::TaxFormResponse**](TaxFormResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/pdf, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
