# \AccountManagementUtilitiesApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**gw_api_v1_enumerations_complex_asset_transfer_get**](AccountManagementUtilitiesApi.md#gw_api_v1_enumerations_complex_asset_transfer_get) | **GET** /gw/api/v1/enumerations/complex-asset-transfer | Get A List Of Participating Brokers For The Given Asset Type
[**gw_api_v1_enumerations_enumeration_type_get**](AccountManagementUtilitiesApi.md#gw_api_v1_enumerations_enumeration_type_get) | **GET** /gw/api/v1/enumerations/{enumerationType} | Get Enumerations
[**gw_api_v1_fee_templates_post**](AccountManagementUtilitiesApi.md#gw_api_v1_fee_templates_post) | **POST** /gw/api/v1/fee-templates | Set Fees For Account
[**gw_api_v1_fee_templates_query_post**](AccountManagementUtilitiesApi.md#gw_api_v1_fee_templates_query_post) | **POST** /gw/api/v1/fee-templates/query | View Fee Template For Account
[**gw_api_v1_forms_get**](AccountManagementUtilitiesApi.md#gw_api_v1_forms_get) | **GET** /gw/api/v1/forms | Get Forms
[**gw_api_v1_participating_banks_get**](AccountManagementUtilitiesApi.md#gw_api_v1_participating_banks_get) | **GET** /gw/api/v1/participating-banks | Get Participating Banks
[**gw_api_v1_requests_get**](AccountManagementUtilitiesApi.md#gw_api_v1_requests_get) | **GET** /gw/api/v1/requests | Get Requests' Details By Timeframe
[**gw_api_v1_requests_request_id_status_get**](AccountManagementUtilitiesApi.md#gw_api_v1_requests_request_id_status_get) | **GET** /gw/api/v1/requests/{requestId}/status | Get Status Of A Request
[**gw_api_v1_validations_usernames_username_get**](AccountManagementUtilitiesApi.md#gw_api_v1_validations_usernames_username_get) | **GET** /gw/api/v1/validations/usernames/{username} | Verify User Availability

## gw_api_v1_enumerations_complex_asset_transfer_get

> models::GetBrokerListResponse gw_api_v1_enumerations_complex_asset_transfer_get(client_id, instruction_type)
Get A List Of Participating Brokers For The Given Asset Type

Get list of brokers supported for given asset transfer type<br><br>**Scope**: `enumerations.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**instruction_type** | **String** | Asset transfer type to get the list of supported brokers | [required] |

### Return type

[**models::GetBrokerListResponse**](GetBrokerListResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_enumerations_enumeration_type_get

> models::EnumerationResponse gw_api_v1_enumerations_enumeration_type_get(r#type, currency, ib_entity, md_status_non_pro, form_number, language)
Get Enumerations

Used to query list of enumerations for attributes included within extPositionsTransfers, occupation, employerBusiness, financialInformation, affiliationDetails, tradingPermissions, etc.<br><br>**Scope**: `accounts.read` OR `enumerations.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**r#type** | [**EnumerationType**](.md) |  | [required] |
**currency** | Option<**String**> |  |  |
**ib_entity** | Option<**String**> |  |  |
**md_status_non_pro** | Option<**String**> |  |  |
**form_number** | Option<**String**> |  |  |
**language** | Option<**String**> |  |  |

### Return type

[**models::EnumerationResponse**](EnumerationResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_fee_templates_post

> models::AsynchronousInstructionResponse gw_api_v1_fee_templates_post(client_id, gw_api_v1_fee_templates_post_request)
Set Fees For Account

Apply predefined fee template to existing account.<br><br>**Scope**: `fee-templates.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_fee_templates_post_request** | [**GwApiV1FeeTemplatesPostRequest**](GwApiV1FeeTemplatesPostRequest.md) |  | [required] |

### Return type

[**models::AsynchronousInstructionResponse**](AsynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_fee_templates_query_post

> models::QueryFeeTemplateResponse gw_api_v1_fee_templates_query_post(client_id, gw_api_v1_fee_templates_query_post_request)
View Fee Template For Account

View fee template applied to an existing account.<br><br>**Scope**: `fee-templates.read`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_fee_templates_query_post_request** | [**GwApiV1FeeTemplatesQueryPostRequest**](GwApiV1FeeTemplatesQueryPostRequest.md) | Create fee template request body | [required] |

### Return type

[**models::QueryFeeTemplateResponse**](QueryFeeTemplateResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_forms_get

> models::FormFileResponse gw_api_v1_forms_get(form_no, get_docs, from_date, to_date, language, projection)
Get Forms

Get forms<br><br>**Scope**: `accounts.read` OR `forms.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**form_no** | Option<[**Vec<i32>**](i32.md)> |  |  |
**get_docs** | Option<**String**> |  |  |
**from_date** | Option<**String**> |  |  |
**to_date** | Option<**String**> |  |  |
**language** | Option<**String**> |  |  |
**projection** | Option<**String**> |  |  |[default to NONE]

### Return type

[**models::FormFileResponse**](FormFileResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_participating_banks_get

> models::GetParticipatingListResponse gw_api_v1_participating_banks_get(client_id, r#type)
Get Participating Banks

Get list of banks which support banking connection with Interactive Brokers.<br><br>**Scope**: `enumerations.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**r#type** | **String** | Parameter for which the list of participating banks is fetched | [required] |

### Return type

[**models::GetParticipatingListResponse**](GetParticipatingListResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_requests_get

> models::RequestDetailsResponse gw_api_v1_requests_get(request_details)
Get Requests' Details By Timeframe

Fetch Requests' Details By Timeframe<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**request_details** | [**RequestDetailsRequest**](.md) |  | [required] |

### Return type

[**models::RequestDetailsResponse**](RequestDetailsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_requests_request_id_status_get

> models::GwApiV1RequestsRequestIdStatusGet200Response gw_api_v1_requests_request_id_status_get(request_id, r#type)
Get Status Of A Request

Returns status for account management request<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**request_id** | **i32** |  | [required] |
**r#type** | **String** |  | [required] |

### Return type

[**models::GwApiV1RequestsRequestIdStatusGet200Response**](_gw_api_v1_requests__requestId__status_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_validations_usernames_username_get

> models::UserNameAvailableResponse gw_api_v1_validations_usernames_username_get(username)
Verify User Availability

Verify whether user is valid and available<br><br>**Scope**: `accounts.read` OR `validations.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**username** | **String** |  | [required] |

### Return type

[**models::UserNameAvailableResponse**](UserNameAvailableResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
