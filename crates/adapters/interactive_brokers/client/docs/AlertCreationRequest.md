# AlertCreationRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**order_id** | Option<**i64**> | optional; used in case of modification and represent Alert Id | [optional]
**alert_name** | **String** | Alert name. |
**alert_message** | **String** | Alert message which will be sent |
**alert_repeatable** | **i32** | Boolean number (0, 1) signifies if an alert can be triggered more than once. A value of ‘1’ is required for MTA alerts |
**email** | Option<**String**> | Email address you want to send email alerts to | [optional]
**expire_time** | Option<**String**> | Used with a tif of “GTD” only. Signifies time when the alert should terminate if no alert is triggered. | [optional]
**i_tws_orders_only** | Option<**i32**> | allow (0) or disallow (1) alerts to trigger alerts through the mobile app | [optional]
**outside_rth** | **i32** | Allow (1) or disallow (0) the alert to be triggered outside of regular trading hours |
**send_message** | Option<**i32**> | allow (1) or disallow (0) alerts to trigger email messages | [optional]
**show_popup** | Option<**i32**> | allow (1) or disallow (0) alerts to trigger TWS Pop-up messages | [optional]
**tif** | **String** | Time in Force duration of alert. |
**conditions** | **Vec<String>** | Container for all conditions applied for an alert to trigger. |

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
