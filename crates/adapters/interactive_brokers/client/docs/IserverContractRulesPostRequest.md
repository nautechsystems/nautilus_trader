# IserverContractRulesPostRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**conid** | Option<**i32**> | Contract identifier for the interested contract. | [optional]
**is_buy** | Option<**bool**> | Side of the market rules apply too. Set to true for Buy Orders, set to false for Sell orders. | [optional][default to true]
**modify_order** | Option<**bool**> | Used to find trading rules related to an existing order. | [optional][default to false]
**order_id** | Option<**i32**> | Specify the order identifier used for tracking a given order. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
