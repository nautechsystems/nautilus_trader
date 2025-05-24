# IserverHistoryResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**server_id** | Option<**String**> | Internal use. Identifier of the request. | [optional]
**symbol** | Option<**String**> | Symbol of the request instrument. | [optional]
**text** | Option<**String**> | Description or company name of the instrument. | [optional]
**price_factor** | Option<**i32**> | Internal use. Used to scale Client Portal chart Y-axis. | [optional]
**start_time** | Option<**String**> | UTC date and time of the start (chronologically earlier) of the complete period in format YYYYMMDD-hh:mm:ss. | [optional]
**high** | Option<**String**> | Internal use. Delivers highest price value in total interval. Used for chart scaling. A string constructed as 'highestPrice*priceFactor/totalVolume*volumeFactor/minutesFromStartTime'. | [optional]
**low** | Option<**String**> | Internal use. Delivers lowest price value in total interval. Used for chart scaling. A string constructed as 'lowestPrice*priceFactor/totalVolume*volumeFactor/minutesFromStartTime'. | [optional]
**time_period** | Option<**String**> | The client-specified period value. | [optional]
**bar_length** | Option<**i32**> | The client-specified bar width, represented in seconds. | [optional]
**md_availability** | Option<**String**> | A three-character string reflecting the nature of available data. R = Realtime, D = Delayed, Z = Frozen, Y = Frozen Delayed, N = Not Subscribed. P = Snapshot, p = Consolidated. B = Top of book. | [optional]
**outside_rth** | Option<**bool**> | Indicates whether data from outside regular trading hours is included in the response. | [optional]
**trading_day_duration** | Option<**i32**> | Length of instrument's trading day in seconds. | [optional]
**volume_factor** | Option<**i32**> | Internal use. Used to scale volume histograms. | [optional]
**price_display_rule** | Option<**i32**> | Internal use. Governs application of pricing display rule. | [optional]
**price_display_value** | Option<**String**> | Internal use. Governs rendering of displayed pricing. | [optional]
**chart_pan_start_time** | Option<**String**> | Internal use. UTC datetime string used to center Client Portal charts. Format YYYYMMDD-hh:mm:ss. | [optional]
**direction** | Option<**i32**> | Indicates how the period is applied in relation to the startTime. Value will always be -1, indicating that the period extends from the startTime forward into the future. | [optional]
**negative_capable** | Option<**bool**> | Indicates whether instrument is capable of negative pricing. | [optional]
**message_version** | Option<**i32**> | Internal use. Reflects the version of the response schema used. | [optional]
**travel_time** | Option<**i32**> | Internal time in flight to serve the request. | [optional]
**data** | Option<[**Vec<models::SingleHistoricalBar>**](singleHistoricalBar.md)> | Array containing OHLC bars for the requested period. | [optional]
**points** | Option<**i32**> | Count of the number of bars returned in the data array. | [optional]
**mkt_data_delay** | Option<**i32**> | Number of milliseconds taken to satisfy this historical data request. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
