# IserverSecdefSearchPostRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**symbol** | **String** | The ticker symbol, bond issuer type, or company name of the equity you are looking to trade. |
**sec_type** | Option<**String**> | Available underlying security types:   * `STK` - Represents an underlying as a Stock security type.   * `IND` - Represents an underlying as an Index security type.   * `BOND` - Represents an underlying as a Bond security type.  | [optional][default to Stk]
**name** | Option<**bool**> | Denotes if the symbol value is the ticker symbol or part of the company's name. | [optional]
**more** | Option<**bool**> |  | [optional]
**fund** | Option<**bool**> | fund search | [optional]
**fund_family_conid_ex** | Option<**String**> |  | [optional]
**pattern** | Option<**bool**> | pattern search | [optional]
**referrer** | Option<**String**> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
