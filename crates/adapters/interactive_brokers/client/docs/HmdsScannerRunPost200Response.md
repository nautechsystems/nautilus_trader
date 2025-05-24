# HmdsScannerRunPost200Response

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**total** | Option<**i32**> | Returns the total number of bonds that match the indicated search. | [optional]
**size** | Option<**i32**> | Returns the total size of the return. | [optional]
**offset** | Option<**i32**> | Returns the distance displaced from the starting 0 value. | [optional]
**scan_time** | Option<**String**> | Returns the UTC datetime value of when the request took place. | [optional]
**id** | Option<**String**> | Returns the market scanner name. Automatically generates an incremental scanner name for each request formatted as \"scanner{ N }\" | [optional]
**position** | Option<**String**> | (Internal use only) | [optional]
**warning_text** | Option<**String**> | Returns the number of contracts returned out of total contracts that match. | [optional]
**contracts** | Option<[**models::HmdsScannerRunPost200ResponseContracts**](_hmds_scanner_run_post_200_response_Contracts.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
