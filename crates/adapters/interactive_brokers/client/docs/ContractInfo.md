# ContractInfo

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**cfi_code** | Option<**String**> | Classification of Financial Instrument codes | [optional]
**symbol** | Option<**String**> | Underlying symbol | [optional]
**cusip** | Option<**String**> | Returns the CUSIP for the given instrument. Only used in BOND trading. | [optional]
**expiry_full** | Option<**String**> | Returns the expiration month of the contract. | [optional]
**con_id** | Option<**i32**> | Indicates the contract identifier of the given contract. | [optional]
**maturity_date** | Option<**String**> | Indicates the final maturity date of the given contract. | [optional]
**industry** | Option<**String**> | Specific group of companies or businesses. | [optional]
**instrument_type** | Option<**String**> | Asset class of the instrument. | [optional]
**trading_class** | Option<**String**> | Designated trading class of the contract. | [optional]
**valid_exchanges** | Option<**String**> | Comma separated list of support exchanges or trading venues. | [optional]
**allow_sell_long** | Option<**bool**> | Allowed to sell shares you own. | [optional]
**is_zero_commission_security** | Option<**bool**> | Indicates if the contract supports zero commission trading. | [optional]
**local_symbol** | Option<**String**> | Contract’s symbol from primary exchange. For options it is the OCC symbol. | [optional]
**contract_clarification_type** | Option<**String**> |  | [optional]
**classifier** | Option<**String**> |  | [optional]
**currency** | Option<**String**> | Base currency contract is traded in. | [optional]
**text** | Option<**String**> | Indicates the display name of the contract, as shown with Client Portal. | [optional]
**underlying_con_id** | Option<**i32**> | Underlying contract identifier for the requested contract. | [optional]
**r_t_h** | Option<**bool**> | Indicates if the contract can be traded outside regular trading hours or not. | [optional]
**multiplier** | Option<**String**> | Indicates the multiplier of the contract. | [optional]
**underlying_issuer** | Option<**String**> | Indicates the issuer of the underlying. | [optional]
**contract_month** | Option<**String**> | Indicates the year and month the contract expires. | [optional]
**company_name** | Option<**String**> | Indicates the name of the company or index. | [optional]
**smart_available** | Option<**bool**> | Indicates if the contract can be smart routed or not. | [optional]
**exchange** | Option<**String**> | Indicates the primary exchange for which the contract can be traded. | [optional]
**category** | Option<**String**> | Indicates the industry category of the instrument. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
