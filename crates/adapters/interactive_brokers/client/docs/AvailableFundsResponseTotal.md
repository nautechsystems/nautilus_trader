# AvailableFundsResponseTotal

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**current_available** | Option<**String**> | Describes currently available funds in your account for trading. | [optional]
**current_excess** | Option<**String**> | Describes total value of the account. | [optional]
**prdctd_pst_xpry_excss** | Option<**String**> | Displays predicted post-expiration account value. | [optional]
**lk_ahd_avlbl_fnds** | Option<**String**> | This value reflects your available funds at the next margin change. | [optional]
**lk_ahd_excss_lqdty** | Option<**String**> | * `Securities` - Equity with loan value. Look ahead maintenance margin.  * `Commodities` - Net Liquidation value. Look ahead maintenance margin.  | [optional]
**overnight_available** | Option<**String**> | Describes available funds for overnight trading. | [optional]
**overnight_excess** | Option<**String**> | Overnight refers to the window of time after the local market trading day is closed.    * `Securities` - Equivalent to regular trading hours.     * `Commodities` - Commodities Net Liquidation value. Overnight Maintenance margin.  | [optional]
**buying_power** | Option<**String**> | Describes the total buying power of the account including existing balance with margin. | [optional]
**leverage** | Option<**String**> | Describes the total combined leverage. | [optional]
**lk_ahd_nxt_chng** | Option<**String**> | Describes when the next 'Look Ahead' calculation will take place. | [optional]
**day_trades_left** | Option<**String**> | Describes the number of trades remaining before flagging the Pattern Day Trader status. \"Unlimited\" is used for existing Pattern Day Traders. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
