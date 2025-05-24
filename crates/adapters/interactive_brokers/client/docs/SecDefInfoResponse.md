# SecDefInfoResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**conid** | Option<**i32**> | Contract Identifier of the given contract. | [optional]
**ticker** | Option<**String**> | Ticker symbol for the given contract | [optional]
**sec_type** | Option<**String**> | Security type for the given contract. | [optional]
**listing_exchange** | Option<**String**> | Primary listing exchange for the given contract. | [optional]
**exchange** | Option<**String**> | Exchange requesting data for. | [optional]
**company_name** | Option<**String**> | Name of the company for the given contract. | [optional]
**currency** | Option<**String**> | Traded currency allowed for the given contract. | [optional]
**valid_exchanges** | Option<**String**> | Series of all valid exchanges the contract can be traded on in a single comma-separated string. | [optional]
**price_rendering** | Option<[**serde_json::Value**](.md)> |  | [optional]
**maturity_date** | Option<**String**> | Date of expiration for the given contract. | [optional]
**right** | Option<**String**> | Set the right for the given contract. * `C` - for Call options. * `P` - for Put options.  | [optional]
**strike** | Option<**f64**> | Returns the given strike value for the given contract. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
