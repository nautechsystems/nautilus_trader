# IserverNotificationPostRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**order_id** | Option<**i32**> | IB-assigned order identifier obtained from the ntf websocket message that delivered the server prompt. | [optional]
**req_id** | Option<**String**> | IB-assigned request identifier obtained from the ntf websocket message that delivered the server prompt. | [optional]
**text** | Option<**String**> | The selected value from the \"options\" array delivered in the server prompt ntf websocket message. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
