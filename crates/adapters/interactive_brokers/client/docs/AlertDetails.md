# AlertDetails

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**account** | Option<**String**> | Requestor’s account ID | [optional]
**order_id** | Option<**i32**> | Alert’s tracking ID. Can be used for modifying or deleting alerts. | [optional]
**alert_name** | Option<**String**> | Human readable name of the alert. | [optional]
**tif** | Option<**String**> | Time in Force effective for the Alert | [optional]
**expire_time** | Option<**String**> | Returns the UTC formatted date used in GTD orders. | [optional]
**alert_active** | Option<**i32**> | Returns if the alert is active [1] or disabled [0]. | [optional]
**alert_repeatable** | Option<**i32**> | Returns if the alert can be sent more than once. | [optional]
**alert_email** | Option<**String**> | Returns the designated email address for sendMessage functionality. | [optional]
**alert_send_message** | Option<**i32**> | Returns whether or not the alert will send an email. | [optional]
**alert_message** | Option<**String**> | Returns the body content of what your alert will report once triggered | [optional]
**alert_show_popup** | Option<**i32**> | Returns whether or not the alert will trigger TWS Pop-up messages | [optional]
**alert_play_audio** | Option<**i32**> | Returns whether or not the alert will play audio | [optional]
**order_status** | Option<**String**> | represent order statusAlways returns “Presubmitted”. | [optional]
**alert_triggered** | Option<**i32**> | Returns whether or not the alert was triggered yet. | [optional]
**fg_color** | Option<**String**> | Foreground color. Not applicable to API. | [optional]
**bg_color** | Option<**String**> | Background color. Not applicable to API. | [optional]
**order_not_editable** | Option<**bool**> | Returns if the order can be edited. | [optional]
**itws_orders_only** | Option<**i32**> | Returns whether or not the alert will trigger mobile notifications. | [optional]
**alert_mta_currency** | Option<**String**> | Returns currency set for MTA alerts. Only valid for alert type 8 & 9. | [optional]
**alert_mta_defaults** | Option<**String**> | Returns current MTA default values. | [optional]
**tool_id** | Option<**i32**> | Tracking ID for MTA alerts only. Returns ‘null’ for standard alerts. | [optional]
**time_zone** | Option<**String**> | Returned for time-specific conditions. | [optional]
**alert_default_type** | Option<**i32**> | Returns default type set for alerts. Configured in Client Portal. | [optional]
**condition_size** | Option<**i32**> | Returns the total number of conditions in the alert. | [optional]
**condition_outside_rth** | Option<**i32**> | Returns whether or not the alert will trigger outside of regular trading hours. | [optional]
**conditions** | Option<[**Vec<models::AlertCondition>**](alertCondition.md)> | Returns all conditions | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
