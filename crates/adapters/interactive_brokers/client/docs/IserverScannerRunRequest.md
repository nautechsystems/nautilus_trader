# IserverScannerRunRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**instrument** | Option<**String**> | Instrument type as the target of the market scanner request. Found in the “instrument_list” section of the /iserver/scanner/params response. | [optional]
**r#type** | Option<**String**> | Scanner value the market scanner is sorted by. Based on the “scan_type_list” section of the /iserver/scanner/params response. | [optional]
**location** | Option<**String**> | Location value the market scanner is searching through. Based on the “location_tree” section of the /iserver/scanner/params response. | [optional]
**filter** | Option<[**Vec<models::IserverScannerRunRequestFilterInner>**](iserverScannerRunRequest_filter_inner.md)> | Contains any additional filters that should apply to response. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
