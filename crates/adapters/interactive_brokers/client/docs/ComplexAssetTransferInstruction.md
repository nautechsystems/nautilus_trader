# ComplexAssetTransferInstruction

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**client_instruction_id** | **f64** |  |
**direction** | **String** |  |
**account_id** | **String** |  |
**account_id_at_current_broker** | Option<**String**> |  | [optional]
**quantity** | **f64** |  |
**trading_instrument** | Option<[**serde_json::Value**](serde_json::Value.md)> |  |
**contra_broker_info** | [**models::ContraBrokerInfo**](ContraBrokerInfo.md) |  |
**non_disclosed_detail** | Option<[**models::NonDisclosedDetail**](NonDisclosedDetail.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
