# TransactionsResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**rc** | Option<**i32**> | Client portal use only | [optional]
**nd** | Option<**i32**> | Client portal use only | [optional]
**rpnl** | Option<[**models::TransactionsResponseRpnl**](transactionsResponse_rpnl.md)> |  | [optional]
**currency** | Option<**String**> | Returns the currency the account is traded in. | [optional]
**from** | Option<**i32**> | Returns the epoch time for the start of requests. | [optional]
**id** | Option<**String**> | Returns the request identifier, getTransactions. | [optional]
**to** | Option<**i32**> | Returns the epoch time for the end of requests. | [optional]
**includes_real_time** | Option<**bool**> | Returns if the trades are up to date or not. | [optional]
**transactions** | Option<[**Vec<models::TransactionsResponseTransactionsInner>**](transactionsResponse_transactions_inner.md)> | Lists all supported transaction values. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
