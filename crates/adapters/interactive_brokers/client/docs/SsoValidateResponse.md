# SsoValidateResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**user_id** | Option<**i32**> | Internal user identifier. | [optional]
**user_name** | Option<**String**> | current username logged in for the session. | [optional]
**result** | Option<**bool**> | Confirms if validation was successful. True if session was validated; false if not. | [optional]
**auth_time** | Option<**i32**> | Returns the time of authentication in epoch time. | [optional]
**sf_enabled** | Option<**bool**> | (Internal use only) | [optional]
**is_free_trial** | Option<**bool**> | Returns if the account is a trial account or a funded account. | [optional]
**credential** | Option<**String**> | Returns the underlying username of the account. | [optional]
**ip** | Option<**String**> | Internal use only. Does not reflect the IP address of the user. | [optional]
**expires** | Option<**i32**> | Returns the time until SSO session expiration in milliseconds. | [optional]
**qualified_for_mobile_auth** | Option<**bool**> | Returns if the customer requires two factor authentication. | [optional]
**landing_app** | Option<**String**> | Used for Client Portal (Internal use only) | [optional]
**is_master** | Option<**bool**> | Returns whether the account is a master account (true) or subaccount (false). | [optional]
**last_accessed** | Option<**i32**> | Returns the last time the user was accessed in epoch time. | [optional]
**login_type** | Option<**i32**> | Returns the login type. 1 for Live, 2 for Paper | [optional]
**paper_user_name** | Option<**String**> | Returns the paper username for the account. | [optional]
**features** | Option<[**models::SsoValidateResponseFeatures**](ssoValidateResponse_features.md)> |  | [optional]
**region** | Option<**String**> | Returns the region connected to internally. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
