# WatchlistsResponseData

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**scanners_only** | Option<**bool**> | Indicates if query results contain only market scanners. | [optional]
**show_scanners** | Option<**bool**> | Indicates if market scanners are included in query results. | [optional]
**bulk_delete** | Option<**bool**> | Indicates if username's watchlists can be bulk-deleted. | [optional]
**user_lists** | Option<[**Vec<models::WatchlistsResponseDataUserListsInner>**](watchlistsResponse_data_user_lists_inner.md)> | Array of objects detailing the watchlists saved for the username in use in the current Web API session. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
