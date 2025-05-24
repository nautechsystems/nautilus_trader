# LiveOrdersResponseOrdersInner

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**acct** | Option<**String**> | IB account ID to which the order was placed. | [optional]
**exchange** | Option<**String**> | Routing destination of the order ticket. | [optional]
**conidex** | Option<**String**> | Contract ID and routing destination in format 123456@EXCHANGE. | [optional]
**conid** | Option<**String**> | Contract ID of the order's instrument. | [optional]
**account** | Option<**String**> | IB account ID to which the order was placed. | [optional]
**order_id** | Option<**i32**> | IB-assigned order identifier. | [optional]
**cash_ccy** | Option<**String**> | Currency of the order ticket's Cash Quantity, if applicable. | [optional]
**size_and_fills** | Option<**String**> | Human-readable shorthand rendering of the filled and total quantities of the order. | [optional]
**order_desc** | Option<**String**> | Human-readable shorthand rendering of the order ticket. | [optional]
**description1** | Option<**String**> | Descriptive text, or additional details that specific the instrument. | [optional]
**ticker** | Option<**String**> | Symbol or base product code of the instrument. | [optional]
**sec_type** | Option<**String**> | Asset class of the instrument. | [optional]
**listing_exchange** | Option<**String**> | Exchange on which the instrument is listed. | [optional]
**remaining_quantity** | Option<**String**> | Quantity remaining to be filled in units of the instrument. | [optional]
**filled_quantity** | Option<**String**> | Quantity filled in units of the instrument. | [optional]
**total_size** | Option<**String**> | Total size of an order in the instrument's units. | [optional]
**total_cash_size** | Option<**String**> | Total size of a cash quantity order. | [optional]
**company_name** | Option<**String**> | Name of business associated with instrument, or otherwise description of instrument. | [optional]
**status** | Option<**String**> | Status of the order ticket. | [optional]
**order_ccp_status** | Option<**String**> | IB internal order status. | [optional]
**orig_order_type** | Option<**String**> | Order type of a filled order. | [optional]
**supports_tax_opt** | Option<**String**> | Indicates whether the order is supported by IB's Tax Optimization tool. | [optional]
**last_execution_time** | Option<**String**> | Time of last execution against the order in format YYMMDDhhmmss. | [optional]
**order_type** | Option<**String**> | Order type of a working order ticket. | [optional]
**bg_color** | Option<**String**> | Internal use. IB's UI background color in hex. | [optional]
**fg_color** | Option<**String**> | Internal use. IB's UI foreground color in hex. | [optional]
**is_event_trading** | Option<**String**> | Indicates whether the order ticket is an Event Trading order. | [optional]
**price** | Option<**String**> | Price of the order, if applicable to the order type. | [optional]
**time_in_force** | Option<**String**> | Time of force of the order. | [optional]
**last_execution_time_r** | Option<**String**> | Unix timestamp of the last execution against the order. | [optional]
**side** | Option<**String**> | Side of the order. | [optional]
**avg_price** | Option<**String**> | Average price of fills against the order, if any. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
