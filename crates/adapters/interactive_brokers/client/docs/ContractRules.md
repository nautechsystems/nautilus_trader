# ContractRules

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**algo_eligible** | Option<**bool**> | Indicates if the contract can trade algos or not. | [optional]
**overnight_eligible** | Option<**bool**> | Indicates if outsideRTH trading is permitted for the instrument | [optional]
**cost_report** | Option<**bool**> | Indicates whether or not a cost report has been requested (Client Portal only). | [optional]
**can_trade_acct_ids** | Option<**Vec<String>**> | Indicates permitted accountIDs that may trade the contract. | [optional]
**error** | Option<**String**> | If rules information can not be received for any reason, it will be expressed here. | [optional]
**order_types** | Option<**Vec<String>**> | Indicates permitted order types for use with standard quantity trading. | [optional]
**ib_algo_types** | Option<**Vec<String>**> | Indicates permitted algo types for use with the given contract. | [optional]
**fraq_types** | Option<**Vec<String>**> | Indicates permitted order types for use with fractional trading. | [optional]
**force_order_preview** | Option<**bool**> | Indicates if the order preview is forced upon the user before submission. | [optional]
**cqt_types** | Option<**Vec<String>**> | Indicates accepted order types for use with cash quantity. | [optional]
**order_defaults** | Option<[**models::ContractRulesOrderDefaults**](contractRules_orderDefaults.md)> |  | [optional]
**order_types_outside** | Option<**Vec<String>**> | Indicates permitted order types for use outside of regular trading hours. | [optional]
**default_size** | Option<**i32**> | Default total quantity value for orders. | [optional]
**cash_size** | Option<**i32**> | Default cash value quantity. | [optional]
**size_increment** | Option<**i32**> | Indicates quantity increase for the contract. | [optional]
**tif_types** | Option<**Vec<String>**> | Indicates allowed tif types supported for the contract. | [optional]
**tif_defaults** | Option<[**models::ContractRulesTifDefaults**](contractRules_tifDefaults.md)> |  | [optional]
**limit_price** | Option<**i32**> | Default limit price for the given contract. | [optional]
**stop_price** | Option<**i32**> | Default stop price for the given contract. | [optional]
**order_origination** | Option<**String**> | Order origin designation for US securities options and Options Clearing Corporation | [optional]
**preview** | Option<**bool**> | Indicates if the order preview is required (for client portal only) | [optional]
**display_size** | Option<**i32**> | Standard display increment rule for the instrument. | [optional]
**fraq_int** | Option<**i32**> | Indicates decimal places for fractional order size. | [optional]
**cash_ccy** | Option<**String**> | Indicates base currency for the instrument. | [optional]
**cash_qty_incr** | Option<**i32**> | Indicates cash quantity increment rules. | [optional]
**price_magnifier** | Option<**i32**> | Signifies the magnifier of a given contract. This is separate from the price multiplier, and will typically return ‘null’  | [optional]
**negative_capable** | Option<**bool**> | Indicates if the value of the contract can be negative (true) or if it is always positive (false). | [optional]
**increment_type** | Option<**i32**> | Indicates the type of increment style. | [optional]
**increment_rules** | Option<[**Vec<models::ContractRulesIncrementRulesInner>**](contractRules_incrementRules_inner.md)> | Indicates increment rule values including lowerEdge and increment value. | [optional]
**has_secondary** | Option<**bool**> |  | [optional]
**mod_types** | Option<**Vec<String>**> | Lists the available order types supported when modifying the order. | [optional]
**increment** | Option<**i32**> | Minimum increment values for prices | [optional]
**increment_digits** | Option<**i32**> | Number of decimal places to indicate the increment value. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
