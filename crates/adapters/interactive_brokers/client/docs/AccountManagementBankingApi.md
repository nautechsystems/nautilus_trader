# \AccountManagementBankingApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**gw_api_v1_bank_instructions_post**](AccountManagementBankingApi.md#gw_api_v1_bank_instructions_post) | **POST** /gw/api/v1/bank-instructions | Manage Bank Instructions
[**gw_api_v1_bank_instructions_query_post**](AccountManagementBankingApi.md#gw_api_v1_bank_instructions_query_post) | **POST** /gw/api/v1/bank-instructions/query | View Bank Instructions
[**gw_api_v1_client_instructions_client_instruction_id_get**](AccountManagementBankingApi.md#gw_api_v1_client_instructions_client_instruction_id_get) | **GET** /gw/api/v1/client-instructions/{clientInstructionId} | Get Status For ClientInstructionId
[**gw_api_v1_external_asset_transfers_post**](AccountManagementBankingApi.md#gw_api_v1_external_asset_transfers_post) | **POST** /gw/api/v1/external-asset-transfers | Transfer Positions Externally (ACATS, ATON, FOP, DWAC, Complex Asset Transfer)
[**gw_api_v1_external_cash_transfers_post**](AccountManagementBankingApi.md#gw_api_v1_external_cash_transfers_post) | **POST** /gw/api/v1/external-cash-transfers | Transfer Cash Externally
[**gw_api_v1_external_cash_transfers_query_post**](AccountManagementBankingApi.md#gw_api_v1_external_cash_transfers_query_post) | **POST** /gw/api/v1/external-cash-transfers/query | View Cash Balances
[**gw_api_v1_instruction_sets_instruction_set_id_get**](AccountManagementBankingApi.md#gw_api_v1_instruction_sets_instruction_set_id_get) | **GET** /gw/api/v1/instruction-sets/{instructionSetId} | Get Status For InstructionSetId
[**gw_api_v1_instructions_cancel_post**](AccountManagementBankingApi.md#gw_api_v1_instructions_cancel_post) | **POST** /gw/api/v1/instructions/cancel | Cancel Request
[**gw_api_v1_instructions_instruction_id_get**](AccountManagementBankingApi.md#gw_api_v1_instructions_instruction_id_get) | **GET** /gw/api/v1/instructions/{instructionId} | Get Status For InstructionId
[**gw_api_v1_instructions_query_post**](AccountManagementBankingApi.md#gw_api_v1_instructions_query_post) | **POST** /gw/api/v1/instructions/query | Get Transaction History
[**gw_api_v1_internal_asset_transfers_post**](AccountManagementBankingApi.md#gw_api_v1_internal_asset_transfers_post) | **POST** /gw/api/v1/internal-asset-transfers | Transfer Positions Internally
[**gw_api_v1_internal_cash_transfers_post**](AccountManagementBankingApi.md#gw_api_v1_internal_cash_transfers_post) | **POST** /gw/api/v1/internal-cash-transfers | Transfer Cash Internally

## gw_api_v1_bank_instructions_post

> models::AsynchronousInstructionResponse gw_api_v1_bank_instructions_post(client_id, gw_api_v1_bank_instructions_post_request)
Manage Bank Instructions

Create or delete bank instructions by accountId. Only ACH and EDDA are supported for 'Create'.<br><br>**Scope**: `bank-instructions.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_bank_instructions_post_request** | [**GwApiV1BankInstructionsPostRequest**](GwApiV1BankInstructionsPostRequest.md) |  | [required] |

### Return type

[**models::AsynchronousInstructionResponse**](AsynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_bank_instructions_query_post

> models::GwApiV1BankInstructionsQueryPost201Response gw_api_v1_bank_instructions_query_post(client_id, gw_api_v1_bank_instructions_query_post_request)
View Bank Instructions

View active bank instructions for an accountId.<br><br>**Scope**: `bank-instructions.read`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_bank_instructions_query_post_request** | [**GwApiV1BankInstructionsQueryPostRequest**](GwApiV1BankInstructionsQueryPostRequest.md) | Create get instruction name request body | [required] |

### Return type

[**models::GwApiV1BankInstructionsQueryPost201Response**](_gw_api_v1_bank_instructions_query_post_201_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_client_instructions_client_instruction_id_get

> models::GwApiV1ClientInstructionsClientInstructionIdGet200Response gw_api_v1_client_instructions_client_instruction_id_get(client_id, client_instruction_id)
Get Status For ClientInstructionId

Retrieve status of request by clientInstructionId.<br><br>**Scope**: `instructions.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**client_instruction_id** | **i32** | The target instruction id. | [required] |

### Return type

[**models::GwApiV1ClientInstructionsClientInstructionIdGet200Response**](_gw_api_v1_client_instructions__clientInstructionId__get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_external_asset_transfers_post

> models::AsynchronousInstructionResponse gw_api_v1_external_asset_transfers_post(client_id, gw_api_v1_external_asset_transfers_post_request)
Transfer Positions Externally (ACATS, ATON, FOP, DWAC, Complex Asset Transfer)

Initiate request to submit external position transfer. Methods- ACATS, ATON, Basic FOP, FOP, DWAC. More information on transfer methods can be found here - <https://www.interactivebrokers.com/campus/trading-lessons/cash-and-position-transfers/><br><br>**Scope**: `transfers.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_external_asset_transfers_post_request** | [**GwApiV1ExternalAssetTransfersPostRequest**](GwApiV1ExternalAssetTransfersPostRequest.md) |  | [required] |

### Return type

[**models::AsynchronousInstructionResponse**](AsynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_external_cash_transfers_post

> models::AsynchronousInstructionResponse gw_api_v1_external_cash_transfers_post(client_id, gw_api_v1_external_cash_transfers_post_request)
Transfer Cash Externally

Initiate request to deposit or withdrawal between IBKR account and bank account. More information on transfer methods can be found here - <https://www.interactivebrokers.com/campus/trading-lessons/cash-and-position-transfers><br><br>**Scope**: `transfers.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_external_cash_transfers_post_request** | [**GwApiV1ExternalCashTransfersPostRequest**](GwApiV1ExternalCashTransfersPostRequest.md) |  | [required] |

### Return type

[**models::AsynchronousInstructionResponse**](AsynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_external_cash_transfers_query_post

> models::GwApiV1ExternalCashTransfersQueryPost201Response gw_api_v1_external_cash_transfers_query_post(client_id, gw_api_v1_external_cash_transfers_query_post_request)
View Cash Balances

View available cash for withdrawal with and without margin loan by accountId<br><br>**Scope**: `transfers.read`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_external_cash_transfers_query_post_request** | [**GwApiV1ExternalCashTransfersQueryPostRequest**](GwApiV1ExternalCashTransfersQueryPostRequest.md) | Create an external cash transfer query request body | [required] |

### Return type

[**models::GwApiV1ExternalCashTransfersQueryPost201Response**](_gw_api_v1_external_cash_transfers_query_post_201_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_instruction_sets_instruction_set_id_get

> models::BulkMultiStatusResponse gw_api_v1_instruction_sets_instruction_set_id_get(client_id, instruction_set_id)
Get Status For InstructionSetId

Retrieve status of all requests associated with instructionSetId.<br><br>**Scope**: `instructions.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**instruction_set_id** | **i32** | The target instruction set id. | [required] |

### Return type

[**models::BulkMultiStatusResponse**](BulkMultiStatusResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_instructions_cancel_post

> models::SynchronousInstructionResponse gw_api_v1_instructions_cancel_post(client_id, gw_api_v1_instructions_cancel_post_request)
Cancel Request

Cancel request by instructionId.<br><br>**Scope**: `instructions.read`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_instructions_cancel_post_request** | [**GwApiV1InstructionsCancelPostRequest**](GwApiV1InstructionsCancelPostRequest.md) |  | [required] |

### Return type

[**models::SynchronousInstructionResponse**](SynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_instructions_instruction_id_get

> models::GwApiV1ClientInstructionsClientInstructionIdGet200Response gw_api_v1_instructions_instruction_id_get(client_id, instruction_id)
Get Status For InstructionId

Retrieve status of request by instructionId<br><br>**Scope**: `instructions.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**instruction_id** | **i32** | The target instruction id. | [required] |

### Return type

[**models::GwApiV1ClientInstructionsClientInstructionIdGet200Response**](_gw_api_v1_client_instructions__clientInstructionId__get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_instructions_query_post

> models::AsynchronousInstructionResponse gw_api_v1_instructions_query_post(client_id, gw_api_v1_instructions_query_post_request)
Get Transaction History

Query list of recent transactions (up to 30 days) based on accountId.<br><br>**Scope**: `instructions.read`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_instructions_query_post_request** | [**GwApiV1InstructionsQueryPostRequest**](GwApiV1InstructionsQueryPostRequest.md) | Create recent instructions request body | [required] |

### Return type

[**models::AsynchronousInstructionResponse**](AsynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_internal_asset_transfers_post

> models::AsynchronousInstructionResponse gw_api_v1_internal_asset_transfers_post(client_id, gw_api_v1_internal_asset_transfers_post_request)
Transfer Positions Internally

Transfer positions internally between two accounts with Interactive Brokers<br><br>**Scope**: `transfers.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_internal_asset_transfers_post_request** | [**GwApiV1InternalAssetTransfersPostRequest**](GwApiV1InternalAssetTransfersPostRequest.md) |  | [required] |

### Return type

[**models::AsynchronousInstructionResponse**](AsynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_internal_cash_transfers_post

> models::SynchronousInstructionResponse gw_api_v1_internal_cash_transfers_post(client_id, gw_api_v1_internal_cash_transfers_post_request)
Transfer Cash Internally

Transfer cash internally between two accounts with Interactive Brokers.<br><br>**Scope**: `transfers.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_internal_cash_transfers_post_request** | [**GwApiV1InternalCashTransfersPostRequest**](GwApiV1InternalCashTransfersPostRequest.md) |  | [required] |

### Return type

[**models::SynchronousInstructionResponse**](SynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
