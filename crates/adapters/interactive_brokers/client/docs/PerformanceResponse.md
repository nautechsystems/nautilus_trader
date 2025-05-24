# PerformanceResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**currency_type** | Option<**String**> | Confirms if the currency type. If trading exclusively in your base currency, “base” will be returned. | [optional]
**rc** | Option<**i32**> | Returns the data identifier (Internal Use Only). | [optional]
**nav** | Option<[**models::PerformanceResponseNav**](performanceResponse_nav.md)> |  | [optional]
**and** | Option<**i32**> | Returns the total data points. | [optional]
**cps** | Option<[**models::PerformanceResponseCps**](performanceResponse_cps.md)> |  | [optional]
**tpps** | Option<[**models::PerformanceResponseTpps**](performanceResponse_tpps.md)> |  | [optional]
**id** | Option<**String**> | Returns the request identifier, getPerformanceData. | [optional]
**included** | Option<**Vec<String>**> | Returns an array containing accounts reviewed. | [optional]
**pm** | Option<**String**> | Portfolio Measure. Used to indicate TWR or MWR values returned. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
