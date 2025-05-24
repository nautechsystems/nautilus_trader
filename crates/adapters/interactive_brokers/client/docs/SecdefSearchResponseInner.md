# SecdefSearchResponseInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**bondid** | Option<**i32**> | applicable for bonds | [optional]
**conid** | Option<**String**> | Contract identifier for the unique contract. | [optional]
**company_header** | Option<**String**> | Company Name - Exchange | [optional]
**company_name** | Option<**String**> | Formal name of the company. | [optional]
**symbol** | Option<**String**> | Underlying ticker symbol. | [optional]
**description** | Option<**String**> | Primary exchange of the contract | [optional]
**restricted** | Option<**bool**> | Returns if the contract is available for trading. | [optional]
**fop** | Option<**String**> | Returns a string of dates, separated by semicolons. | [optional]
**opt** | Option<**String**> | Returns a string of dates, separated by semicolons. | [optional]
**war** | Option<**String**> | Returns a string of dates, separated by semicolons. | [optional]
**sections** | Option<[**Vec<models::SecdefSearchResponseInnerSectionsInner>**](secdefSearchResponse_inner_sections_inner.md)> |  | [optional]
**issuers** | Option<[**Vec<models::SecdefSearchResponseInnerIssuersInner>**](secdefSearchResponse_inner_issuers_inner.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
