# BrokerageSessionStatus

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**authenticated** | Option<**bool**> | Returns whether your brokerage session is authenticated or not. | [optional]
**competing** | Option<**bool**> | Returns whether you have a competing brokerage session in another connection. | [optional]
**connected** | Option<**bool**> | Returns whether you are connected to the gateway or not. | [optional]
**message** | Option<**String**> | A message about your authenticate status if any. | [optional]
**mac** | Option<**String**> | Device MAC information. | [optional]
**server_info** | Option<[**models::BrokerageSessionStatusServerInfo**](brokerageSessionStatus_serverInfo.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
