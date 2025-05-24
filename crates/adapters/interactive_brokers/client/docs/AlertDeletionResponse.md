# AlertDeletionResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**request_id** | Option<**i32**> | Not applicable | [optional]
**order_id** | Option<**i32**> | The tracking number of the alert. Occasssionally referenced as the alertId or alert_id.  | [optional]
**success** | Option<**bool**> | Displays result status of alert request | [optional]
**text** | Option<**String**> | Response message to clarify success status reason. | [optional]
**failure_list** | Option<**String**> | If “success” returns false, will list failed order Ids | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
