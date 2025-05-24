# DetailedContractInformationValueValue

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**nav** | Option<**Vec<f64>**> | Net asset value data for the account or consolidated accounts. NAV data is not applicable to benchmarks. | [optional]
**cps** | Option<**Vec<f64>**> | Returns the object containing the Cumulative performance data. Correlates to the same index position of data returned by the \"nav\" field. | [optional]
**freq** | Option<**String**> | Returns the determining frequency of the data range. | [optional]
**dates** | Option<**Vec<String>**> | Returns the dates corresponding to the frequency of data. | [optional]
**start_nav** | Option<[**models::DetailedContractInformationValueValueStartNav**](detailedContractInformation_value_value_startNav.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
