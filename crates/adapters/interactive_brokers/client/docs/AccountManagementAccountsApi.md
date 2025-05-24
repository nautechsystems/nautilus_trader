# \AccountManagementAccountsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**gw_api_v1_accounts_account_id_details_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_account_id_details_get) | **GET** /gw/api/v1/accounts/{accountId}/details | Get Account Information
[**gw_api_v1_accounts_account_id_kyc_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_account_id_kyc_get) | **GET** /gw/api/v1/accounts/{accountId}/kyc | Retrieve Au10Tix URL
[**gw_api_v1_accounts_account_id_login_messages_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_account_id_login_messages_get) | **GET** /gw/api/v1/accounts/{accountId}/login-messages | Get Login Message By Account
[**gw_api_v1_accounts_account_id_status_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_account_id_status_get) | **GET** /gw/api/v1/accounts/{accountId}/status | Get Status By Account
[**gw_api_v1_accounts_account_id_tasks_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_account_id_tasks_get) | **GET** /gw/api/v1/accounts/{accountId}/tasks | Get Registration Tasks
[**gw_api_v1_accounts_close_post**](AccountManagementAccountsApi.md#gw_api_v1_accounts_close_post) | **POST** /gw/api/v1/accounts/close | Close Account
[**gw_api_v1_accounts_documents_post**](AccountManagementAccountsApi.md#gw_api_v1_accounts_documents_post) | **POST** /gw/api/v1/accounts/documents | Submit General Agreements And Disclosures
[**gw_api_v1_accounts_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_get) | **GET** /gw/api/v1/accounts | Retrieve Processed Application
[**gw_api_v1_accounts_login_messages_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_login_messages_get) | **GET** /gw/api/v1/accounts/login-messages | Get Login Messages
[**gw_api_v1_accounts_patch**](AccountManagementAccountsApi.md#gw_api_v1_accounts_patch) | **PATCH** /gw/api/v1/accounts | Update Account
[**gw_api_v1_accounts_post**](AccountManagementAccountsApi.md#gw_api_v1_accounts_post) | **POST** /gw/api/v1/accounts | Create Account
[**gw_api_v1_accounts_status_get**](AccountManagementAccountsApi.md#gw_api_v1_accounts_status_get) | **GET** /gw/api/v1/accounts/status | Get Status Of Accounts

## gw_api_v1_accounts_account_id_details_get

> models::AccountDetailsResponse gw_api_v1_accounts_account_id_details_get(account_id)
Get Account Information

<br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::AccountDetailsResponse**](AccountDetailsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_account_id_kyc_get

> models::Au10TixDetailResponse gw_api_v1_accounts_account_id_kyc_get(account_id)
Retrieve Au10Tix URL

Generate URL address to complete real-time KYC verification using Au10Tix<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::Au10TixDetailResponse**](Au10TixDetailResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_account_id_login_messages_get

> models::LoginMessageResponse gw_api_v1_accounts_account_id_login_messages_get(account_id, r#type)
Get Login Message By Account

Query login messages assigned by accountId<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**r#type** | Option<**String**> |  |  |

### Return type

[**models::LoginMessageResponse**](LoginMessageResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_account_id_status_get

> models::AccountStatusResponse gw_api_v1_accounts_account_id_status_get(account_id)
Get Status By Account

Query status of account by accountId<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |

### Return type

[**models::AccountStatusResponse**](AccountStatusResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_account_id_tasks_get

> models::GwApiV1AccountsAccountIdTasksGet200Response gw_api_v1_accounts_account_id_tasks_get(account_id, r#type)
Get Registration Tasks

Query registration tasks assigned to account and pending tasks that are required for account approval<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | **String** |  | [required] |
**r#type** | Option<**String**> |  |  |[default to registration]

### Return type

[**models::GwApiV1AccountsAccountIdTasksGet200Response**](_gw_api_v1_accounts__accountId__tasks_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_close_post

> models::AsynchronousInstructionResponse gw_api_v1_accounts_close_post(client_id, gw_api_v1_accounts_close_post_request)
Close Account

Submit request to close account that is opened.<br><br>**Scope**: `accounts.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**client_id** | **String** | The client's clientId | [required] |
**gw_api_v1_accounts_close_post_request** | [**GwApiV1AccountsClosePostRequest**](GwApiV1AccountsClosePostRequest.md) |  | [required] |

### Return type

[**models::AsynchronousInstructionResponse**](AsynchronousInstructionResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/problem+json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_documents_post

> models::StatusResponse gw_api_v1_accounts_documents_post(process_documents_payload)
Submit General Agreements And Disclosures

Provides mechanism to submit Agreements and Disclosures to IBKR once a day instead of with each application. We store these documents on the servers and will use them for new application requests submitted that day.<ul><li>Documents will need to be submitted once a day (before the Applications are submitted). PDFs will be displayed and submitted as is- no changes/edits will be made to the actual PDF files.</li><li>This end-point will not process any Tax Form Documents. Tax Form document should be submitted with every application</li><li>If submitted in the morning, you only need to include the Tax Form attachment for each applicant. Otherwise, you will need to include PDFs with each application (Create Account).</li></ul><br><br>**Scope**: `accounts.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**process_documents_payload** | [**ProcessDocumentsPayload**](ProcessDocumentsPayload.md) |  | [required] |

### Return type

[**models::StatusResponse**](StatusResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/jwt
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_get

> models::GwApiV1AccountsGet200Response gw_api_v1_accounts_get(account_id, external_id)
Retrieve Processed Application

Retrieve the application request and IBKR response data based on IBKR accountId or externalId. Only available for accounts that originate via API<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_id** | Option<**String**> |  |  |
**external_id** | Option<**String**> |  |  |

### Return type

[**models::GwApiV1AccountsGet200Response**](_gw_api_v1_accounts_get_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_login_messages_get

> models::LoginMessageResponse gw_api_v1_accounts_login_messages_get(login_message_request)
Get Login Messages

Query all accounts associated with ‘Client ID’ that have incomplete login message<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**login_message_request** | [**LoginMessageRequest**](.md) |  | [required] |

### Return type

[**models::LoginMessageResponse**](LoginMessageResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_patch

> models::StatusResponse gw_api_v1_accounts_patch(account_management_requests_payload)
Update Account

Update information for an existing accountId<br><br>**Scope**: `accounts.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_management_requests_payload** | [**AccountManagementRequestsPayload**](AccountManagementRequestsPayload.md) |  | [required] |

### Return type

[**models::StatusResponse**](StatusResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/jwt
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_post

> models::StatusResponse gw_api_v1_accounts_post(application_payload)
Create Account

Submit account application. This will create brokerage account for the end user.<br><br>**Scope**: `accounts.write`<br>**Security Policy**: `Signed JWT`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**application_payload** | [**ApplicationPayload**](ApplicationPayload.md) |  | [required] |

### Return type

[**models::StatusResponse**](StatusResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/jwt
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## gw_api_v1_accounts_status_get

> models::AccountStatusBulkResponse gw_api_v1_accounts_status_get(account_status_request)
Get Status Of Accounts

Query status of all accounts associated with ‘Client ID'<br><br>**Scope**: `accounts.read`<br>**Security Policy**: `HTTPS`

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**account_status_request** | [**AccountStatusRequest**](.md) |  | [required] |

### Return type

[**models::AccountStatusBulkResponse**](AccountStatusBulkResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/problem+json, */*, application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
