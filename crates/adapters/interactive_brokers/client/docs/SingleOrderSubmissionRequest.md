# SingleOrderSubmissionRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**acct_id** | Option<**String**> | Receiving account of the order ticket. | [optional]
**conid** | **i32** | IB contract ID of the instrument. |
**conidex** | Option<**String**> | Contract ID and routing destination together in format 123456@EXCHANGE. | [optional]
**sec_type** | Option<**String**> | IB asset class identifier. | [optional]
**c_oid** | Option<**String**> | Client-configurable order identifier. | [optional]
**parent_id** | Option<**String**> | If the order ticket is a child order in a bracket, the parentId field must be set equal to the cOID provided for the parent order. | [optional]
**listing_exchange** | Option<**String**> | The listing exchange of the instrument. | [optional]
**is_single_group** | Option<**bool**> | Indicates that all orders in the containing array are to be treated as an OCA group. | [optional]
**outside_rth** | Option<**bool**> | Instructs IB to permit the order to execute outside of regular trading hours. | [optional]
**aux_price** | Option<**f64**> | Additional price value used in certain order types, such as stop orders. | [optional]
**ticker** | Option<**String**> | Ticker symbol of the instrument. | [optional]
**trailing_amt** | Option<**f64**> | Offset used with Trailing orders. | [optional]
**trailing_type** | Option<**String**> | Specifies the type of trailing used with a Trailing order. | [optional]
**referrer** | Option<**String**> | IB internal identifier for order entry UI element. | [optional]
**cash_qty** | Option<**f64**> | Quantity of currency used with cash quantity orders. | [optional]
**use_adaptive** | Option<**bool**> | Instructs IB to apply the Price Management Algo. | [optional]
**is_ccy_conv** | Option<**bool**> | Indicates that a forex order is for currency conversion and should not entail a virtual forex position in the account, where applicable. | [optional]
**order_type** | **String** | IB order type identifier. |
**price** | Option<**f64**> | Price of the order ticket, where applicable. | [optional]
**side** | **String** | Side of the order ticket. |
**tif** | **String** | Time in force of the order ticket. |
**quantity** | **f64** | Quantity of the order ticket in units of the instrument. |
**strategy** | Option<**String**> | The name of an execution algorithm. | [optional]
**strategy_parameters** | Option<[**models::SingleOrderSubmissionRequestStrategyParameters**](singleOrderSubmissionRequest_strategyParameters.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
