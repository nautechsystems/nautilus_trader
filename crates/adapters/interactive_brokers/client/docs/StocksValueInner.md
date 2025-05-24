# StocksValueInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**name** | Option<**String**> | Full company name for the given contract. | [optional]
**chinese_name** | Option<**String**> | Chinese name for the given company as unicode. | [optional]
**asset_class** | Option<**String**> | Asset class for the given company. | [optional]
**contracts** | Option<[**Vec<models::StocksValueInnerContractsInner>**](stocks_value_inner_contracts_inner.md)> | A series of arrays pertaining to the same company listed by “name”. Typically differentiated based on currency of the primary exchange.  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
