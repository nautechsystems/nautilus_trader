# \TradingWatchlistsApi

All URIs are relative to *<https://api.ibkr.com>*

Method | HTTP request | Description
------------- | ------------- | -------------
[**iserver_watchlist_delete**](TradingWatchlistsApi.md#iserver_watchlist_delete) | **DELETE** /iserver/watchlist | Delete A Specified Watchlist From The Username's Settings.
[**iserver_watchlist_get**](TradingWatchlistsApi.md#iserver_watchlist_get) | **GET** /iserver/watchlist | Retrieve Details Of A Single Watchlist Stored In The Username's Settings.
[**iserver_watchlist_post**](TradingWatchlistsApi.md#iserver_watchlist_post) | **POST** /iserver/watchlist | Create A Watchlist To Monitor A Set Of Instruments.
[**iserver_watchlists_get**](TradingWatchlistsApi.md#iserver_watchlists_get) | **GET** /iserver/watchlists | Retrieve All Saved Watchlists Stored On IB Backend For The Username In Use In The Current Web API Session.

## iserver_watchlist_delete

> models::WatchlistDeleteSuccess iserver_watchlist_delete(id)
Delete A Specified Watchlist From The Username's Settings.

Delete a specified watchlist from the username's settings.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **String** | Watchlist ID of the watchlist to be deleted. | [required] |

### Return type

[**models::WatchlistDeleteSuccess**](watchlistDeleteSuccess.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_watchlist_get

> models::SingleWatchlist iserver_watchlist_get(id)
Retrieve Details Of A Single Watchlist Stored In The Username's Settings.

Retrieve details of a single watchlist stored in the username's settings.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **String** | Watchlist ID of the requested watchlist. | [required] |

### Return type

[**models::SingleWatchlist**](singleWatchlist.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_watchlist_post

> models::IserverWatchlistPost200Response iserver_watchlist_post(iserver_watchlist_post_request)
Create A Watchlist To Monitor A Set Of Instruments.

Create a named watchlist by submitting a set of conids.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**iserver_watchlist_post_request** | [**IserverWatchlistPostRequest**](IserverWatchlistPostRequest.md) | Watchlist contents. | [required] |

### Return type

[**models::IserverWatchlistPost200Response**](_iserver_watchlist_post_200_response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json, application/json; charset=utf-8, text/plain; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

## iserver_watchlists_get

> models::WatchlistsResponse iserver_watchlists_get(sc)
Retrieve All Saved Watchlists Stored On IB Backend For The Username In Use In The Current Web API Session.

Retrieve all saved watchlists stored on IB backend for the username in use in the current Web API session.

### Parameters

Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**sc** | Option<**String**> | Can only be used with value USER_WATCHLIST, which returns only user-created watchlists and excludes those created by IB. |  |

### Return type

[**models::WatchlistsResponse**](watchlistsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json, text/plain; charset=utf-8, application/json; charset=utf-8

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)
