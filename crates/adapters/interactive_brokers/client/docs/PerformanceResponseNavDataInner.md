# PerformanceResponseNavDataInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id_type** | Option<**String**> | Returns how identifiers are determined. | [optional]
**navs** | Option<**Vec<String>**> | Returns sequential data points corresponding to the net asset value between the \"start\" and \"end\" days. | [optional]
**start** | Option<**String**> | Returns the first available date for data. | [optional]
**end** | Option<**String**> | Returns the end of the available frequency. | [optional]
**id** | Option<**String**> | Returns the account identifier. | [optional]
**start_nav** | Option<[**models::PerformanceResponseNavDataInnerStartNav**](performanceResponse_nav_data_inner_startNAV.md)> |  | [optional]
**base_currency** | Option<**String**> | Returns the base currency used in the account. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
