# IserverWatchlistPost200Response

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | Option<**String**> | The submitted watchlist ID. | [optional]
**hash** | Option<**String**> | IB's internal hash of the submitted watchlist. | [optional]
**name** | Option<**String**> | The submitted human-readable watchlist name. | [optional]
**read_only** | Option<**bool**> | Indicates whether watchlist is write-restricted. User-created watchlists will always show false. | [optional]
**instruments** | Option<**Vec<String>**> | Array will always be empty. Contents can be queried via GET /iserver/watchlist?id= | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
