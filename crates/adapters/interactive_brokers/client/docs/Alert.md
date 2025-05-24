# Alert

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**order_id** | Option<**i32**> | The order id (alert id) | [optional]
**account** | Option<**String**> | The account the alert was attributed to. | [optional]
**alert_name** | Option<**String**> | The requested name for the alert. | [optional]
**alert_active** | Option<**i32**> | Determines if the alert is active [1] or not [0] | [optional]
**order_time** | Option<**String**> | UTC-formatted time of the alert’s creation. | [optional]
**alert_triggered** | Option<**bool**> | Confirms if the order is triggered or not. | [optional]
**alert_repeatable** | Option<**i32**> | Confirms if the alert is enabled to occur more than once. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
