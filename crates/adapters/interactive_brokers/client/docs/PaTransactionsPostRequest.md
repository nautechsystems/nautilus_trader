# PaTransactionsPostRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**acct_ids** | Option<**Vec<String>**> | Include each account ID as a string to receive data for. | [optional]
**conids** | Option<**Vec<String>**> | Include contract ID to receive data for.  Conids may be passed as integers or strings. Only supports one contract id at a time.  | [optional]
**currency** | Option<**String**> | Define the currency to display price amounts with. | [optional][default to USD]
**days** | Option<**i32**> | Specify the number of days to receive transaction data for. | [optional][default to 90]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
