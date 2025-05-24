# InternalPositionTransferInstruction

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**client_instruction_id** | **f64** |  |
**source_account_id** | **String** |  |
**target_account_id** | **String** |  |
**transfer_quantity** | **f64** |  |
**trading_instrument** | Option<[**serde_json::Value**](serde_json::Value.md)> |  |
**transfer_price** | Option<**f64**> | If transferPrice is provided then tradeDate and settleDate are also required | [optional]
**trade_date** | Option<**String**> | If tradeDate is provided then settleDate is also required | [optional]
**settle_date** | Option<**String**> | If settleDate is provided then tradeDate is also required | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
