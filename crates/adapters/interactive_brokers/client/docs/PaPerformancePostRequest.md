# PaPerformancePostRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**acct_ids** | Option<**Vec<String>**> | An array of strings containing each account identifier to retrieve performance details for. | [optional]
**period** | Option<**String**> | Specify the period for which the account should be analyzed. Available period lengths:   * `1D` - The last 24 hours.   * `7D` - The last 7 full days.   * `MTD` - Performance since the 1st of the month.   * `1M` - A full calendar month from the last full trade day.   * `3M` - 3 full calendar months from the last full trade day.   * `6M` - 6 full calendar months from the last full trade day.   * `12M` - 12 full calendar month from the last full trade day.   * `YTD` - Performance since January 1st.  | [optional][default to Variant12M]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
