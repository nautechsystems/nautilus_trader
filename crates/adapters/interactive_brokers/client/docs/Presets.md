# Presets

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**group_auto_close_positions** | Option<**bool**> | Determines if allocation groups should prioritize closing positions over equal distribution. | [optional]
**default_method_for_all** | Option<**String**> | Interactive Brokers supports two forms of allocation methods. Allocation methods that have calculations completed by Interactive Brokers, and a set of allocation methods calculated by the user and then specified. IB-computed allocation methods:   * `A` - Available Equity   * `E` - Equal   * `N` - Net Liquidation Value  User-specified allocation methods:   * `C` - Cash Quantity   * `P` - Percentage   * `R` - Ratios   * `S` - Shares  | [optional]
**profiles_auto_close_positions** | Option<**bool**> | Determines if allocation profiles should prioritize closing positions over equal distribution. | [optional]
**strict_credit_check** | Option<**bool**> | Determines if the system should always check user credit before beginning the order process every time, or only at the time of order placement and execution. | [optional]
**group_proportional_allocation** | Option<**bool**> | Determines if the system should keep allocation groups proportional for scaling. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
