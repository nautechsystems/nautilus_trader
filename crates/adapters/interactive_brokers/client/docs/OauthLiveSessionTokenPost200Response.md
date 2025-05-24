# OauthLiveSessionTokenPost200Response

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**diffie_hellman_challenge** | Option<**String**> | Diffie-Hellman challenge value used to compute live session token locally by client. | [optional]
**live_session_token_signature** | Option<**String**> | Signature value used to validate successful client-side computation of live session token. | [optional]
**live_session_token_expiration** | Option<**i32**> | Unix timestamp in milliseconds of time of live session token computation by IB. Live session tokens are valid for 24 hours from this time. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
