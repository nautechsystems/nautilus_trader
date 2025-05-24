# HmdsHistoryResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**start_time** | Option<**String**> | UTC date and time of the start (chronologically earlier) of the complete period in format YYYYMMDD-hh:mm:ss. | [optional]
**start_time_val** | Option<**i32**> | Unix timestamp of the start (chronologically earlier) of the complete period. | [optional]
**end_time** | Option<**String**> | UTC date and time of the end (chronologically later) of the complete period in format YYYYMMDD-hh:mm:ss. | [optional]
**end_time_val** | Option<**i32**> | Unix timestamp of the end (chronologically later) of the complete period. | [optional]
**data** | Option<[**Vec<models::SingleHistoricalBar>**](singleHistoricalBar.md)> | Array containing OHLC bars for the requested period. | [optional]
**points** | Option<**i32**> | Count of the number of bars returned in the data array. | [optional]
**mkt_data_delay** | Option<**i32**> | Number of milliseconds taken to satisfy this historical data request. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
