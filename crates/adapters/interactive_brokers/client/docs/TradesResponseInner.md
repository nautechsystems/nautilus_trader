# TradesResponseInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**execution_id** | Option<**String**> | IB-assigned execution identifier. | [optional]
**symbol** | Option<**String**> | Symbol of the instrument involved in the execution. | [optional]
**supports_tax_opt** | Option<**String**> | Indicates whether the order is supported by IB's Tax Optimization tool. | [optional]
**side** | Option<**String**> | Side of the execution. | [optional]
**order_description** | Option<**String**> | Human-readable description of the outcome of the execution. | [optional]
**trade_time** | Option<**String**> | UTC date and time of the execution in format YYYYMMDD-hh:mm:ss. | [optional]
**trade_time_r** | Option<**i32**> | Unix timestamp of the execution time in milliseconds. | [optional]
**size** | Option<**f64**> | The size of the execution in units of the instrument. | [optional]
**price** | Option<**String**> | The price at which the execution occurred. | [optional]
**order_ref** | Option<**String**> | The client-provided customer order identifier. Specified via cOID during order submission in the Web API. | [optional]
**submitter** | Option<**String**> | The IB username that originated the order ticket against which the execution occurred. | [optional]
**exchange** | Option<**String**> | The exchange or other venue on which the execution occurred. | [optional]
**commission** | Option<**String**> | Commissions incurred by the execution. May also include | [optional]
**net_amount** | Option<**f64**> | net_amount | [optional]
**account** | Option<**String**> | The IB account ID of the account that received the execution. | [optional]
**account_code** | Option<**String**> | The IB account ID of the account that received the execution. | [optional]
**account_allocation_name** | Option<**String**> | The IB account ID of the account that received the execution. | [optional]
**company_name** | Option<**String**> | Name of business associated with instrument, or otherwise description of instrument. | [optional]
**contract_description_1** | Option<**String**> | Human-readable description of the order's instrument. | [optional]
**sec_type** | Option<**String**> | IB asset class identifier. | [optional]
**listing_exchange** | Option<**String**> | The primary exchange on which the instrument is listed. | [optional]
**conid** | Option<**String**> | Contract ID of the order's instrument. | [optional]
**conid_ex** | Option<**String**> | Contract ID and routing destination in format 123456@EXCHANGE. | [optional]
**clearing_id** | Option<**String**> | Identifier of the firm clearing the trade. Value is \"IB\" if account is cleared by Interactive Brokers. | [optional]
**clearing_name** | Option<**String**> | Name of the firm clearing the trade. Value is \"IB\" if account is cleared by Interactive Brokers. | [optional]
**liquidation_trade** | Option<**String**> | Indicates whether the trade is the result of a liquidiation by IB. | [optional]
**is_event_trading** | Option<**String**> | Indicates whether the order ticket is an Event Trading order. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
