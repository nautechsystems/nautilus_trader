# TradingScheduleInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | Option<**String**> | Exchange parameter id | [optional]
**trade_venue_id** | Option<**String**> | Reference on a trade venue of given exchange parameter | [optional]
**exchange** | Option<**String**> | short exchange name | [optional]
**description** | Option<**String**> | exchange description | [optional]
**timezone** | Option<**String**> | References the time zone corresponding to the listed dates and times. | [optional]
**schedules** | Option<**Vec<String>**> | Always contains at least one ‘tradingTime’ and zero or more ‘sessionTime’ tags | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
