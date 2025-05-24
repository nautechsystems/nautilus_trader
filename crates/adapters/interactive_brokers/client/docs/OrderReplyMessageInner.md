# OrderReplyMessageInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | Option<**String**> | The replyId UUID of the order ticket's emitted order reply messages, used to confirm them and proceed. | [optional]
**is_suppressed** | Option<**bool**> | Internal use. Always delivers value 'false'. | [optional]
**message** | Option<**Vec<String>**> | An array containing the human-readable text of all order reply messages emitted for the order ticket. | [optional]
**message_ids** | Option<**Vec<String>**> | An array containing identifiers that categorize the types of order reply messages that have been emitted. Elements of this array are ordered so that indicies match the corresponding human-readable text strings in the 'message' array. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
