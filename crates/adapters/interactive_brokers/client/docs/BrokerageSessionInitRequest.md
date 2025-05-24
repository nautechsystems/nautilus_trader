# BrokerageSessionInitRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**publish** | Option<**bool**> | publish brokerage session token at the same time when brokerage session initialized. If set false, then session token should be published before calling init. Setting true is preferred way. | [optional]
**compete** | Option<**bool**> | Determines if other brokerage sessions should be disconnected to prioritize this connection. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
