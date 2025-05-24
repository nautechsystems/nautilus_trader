# HmdsScannerRunRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**instrument** | Option<**String**> | Specify the type of instrument for the request. Found under the “instrument_list” value of the /hmds/scanner/params request. | [optional]
**locations** | Option<**String**> | Specify the type of location for the request. Found under the “location_tree” value of the /hmds/scanner/params request. | [optional]
**scan_code** | Option<**String**> | Specify the scanner type for the request. Found under the “scan_type_list” value of the /hmds/scanner/params request. | [optional]
**sec_type** | Option<**String**> | Specify the type of security type for the request. Found under the “location_tree” value of the /hmds/scanner/params request. | [optional]
**delayed_locations** | Option<**String**> |  | [optional]
**max_items** | Option<**i32**> | Specify how many items should be returned. | [optional][default to 250]
**filters** | Option<**Vec<String>**> | Array of objects containing all filters upon the scanner request. Content contains a series of key:value pairs. While “filters” must be specified in the body, no content in the array needs to be passed.  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
