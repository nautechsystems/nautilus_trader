# SummaryOfAccountMarginResponseSecurities

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**current_initial** | Option<**String**> | The minimum amount required to open a new position. | [optional]
**prdctd_pst_xpry_mrgn_at__opn** | Option<**String**> | Provides a projected “at expiration” margin value based on the soon-to-expire contracts in your portfolio. | [optional]
**current_maint** | Option<**String**> | The amount of equity required to maintain your positions. | [optional]
**projected_liquidity_inital_margin** | Option<**String**> | Provides a projected \"liquid\" initial margin value based on account liquidation value. | [optional]
**prjctd_lk_ahd_mntnnc_mrgn** | Option<**String**> | If it is 3:00 pm ET, the next calculation you’re looking ahead to is after the close, or the Overnight Initial Margin. If it’s 3:00 am ET, the next calculation will be at the market’s open.  * `Securities` – Projected maintenance margin requirement as of next period’s margin change, in the base currency of the account.   * `Commodities` – Maintenance margin requirement as of next period’s margin change in the base currency of the account based on current margin requirements, which are subject to change. This value depends on when you are viewing your margin requirements.  | [optional]
**projected_overnight_initial_margin** | Option<**String**> | Overnight refers to the window of time after the local market trading day is closed.    * Securities – Projected overnight initial margin requirement in the base currency of the account.    * Commodities – Overnight initial margin requirement in the base currency of the account based on current margin requirements, which are subject to change.  | [optional]
**prjctd_ovrnght_mntnnc_mrgn** | Option<**String**> | Overnight refers to the window of time after the local market trading day is closed.    * `Securities` – Projected overnight maintenance margin requirement in the base currency of the account.    * `Commodities` – Overnight maintenance margin requirement in the base currency of the account based on current margin requirements, which are subject to change.    | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
