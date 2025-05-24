# AdvancedOrderReject

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**order_id** | Option<**i32**> | The order ID assigned by IB to the rejected order ticket. | [optional]
**req_id** | Option<**String**> | IB's internal identifier assigned to the returned message. | [optional]
**dismissable** | Option<**Vec<String>**> | Indicates whether this prompt is dismissable. | [optional]
**text** | Option<**String**> | Human-readable text of the messages emitted by IB in response to order submission. | [optional]
**options** | Option<**Vec<String>**> | Choices available to the client in response to the rejection message. | [optional]
**r#type** | Option<**String**> | The specific type of message returned. | [optional]
**message_id** | Option<**String**> | IB internal identifier for the nature or category of the returned message. | [optional]
**prompt** | Option<**bool**> | Indicates that the message is a prompt offering a set of decisions, one or more of which may permit the rejected order to be resubmitted. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
