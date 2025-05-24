# TransactionsResponseTransactionsInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**date** | Option<**String**> | Returns the human-readable datetime of the transaction. | [optional]
**cur** | Option<**String**> | Returns the currency of the traded instrument. | [optional]
**fx_rate** | Option<**i32**> | Returns the forex conversion rate. | [optional]
**pr** | Option<**i32**> | Returns the price per share of the transaction. | [optional]
**qty** | Option<**i32**> | Returns the total quantity traded. Will display a negative value for sell orders, and a positive value for buy orders.  | [optional]
**acctid** | Option<**String**> | Returns the account which made the transaction. | [optional]
**amt** | Option<**i32**> | Returns the total value of the trade. | [optional]
**conid** | Option<**i32**> | Returns the contract identifier. | [optional]
**r#type** | Option<**String**> | Returns the order side. | [optional]
**desc** | Option<**String**> | Returns the long name for the company. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
