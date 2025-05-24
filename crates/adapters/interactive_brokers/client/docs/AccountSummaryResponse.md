# AccountSummaryResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**account_type** | Option<**String**> | Describes the unique account type. For standard individual accounts, an empty string is returned. | [optional]
**status** | Option<**String**> | If the account is currently non-tradeable, a status message will be dispalyed. | [optional]
**balance** | Option<**i32**> | Returns the total account balance. | [optional]
**sma** | Option<**i32**> | Simple Moving Average of the account. | [optional]
**buying_power** | Option<**i32**> | Total buying power available for the account. | [optional]
**available_funds** | Option<**i32**> | The amount of equity you have available for trading. For both the Securities and Commodities segments, this is calculated as: Equity with Loan Value – Initial Margin. | [optional]
**excess_liquidity** | Option<**i32**> | The amount of cash in excess of the usual requirement in your account. | [optional]
**net_liquidation_value** | Option<**i32**> | The basis for determining the price of the assets in your account. | [optional]
**equity_with_loan_value** | Option<**i32**> | The basis for determining whether you have the necessary assets to either initiate or maintain security assets. | [optional]
**reg_t_loan** | Option<**i32**> | The Federal Reserve Board regulation governing the amount of credit that broker dealers may extend to clients who borrow money to buy securities on margin. | [optional]
**securities_gvp** | Option<**i32**> | Absolute value of the Long Stock Value + Short Stock Value + Long Option Value + Short Option Value + Fund Value. | [optional]
**total_cash_value** | Option<**i32**> | Cash recognized at the time of trade + futures P&L. This value reflects real-time currency positions, including:  *  Trades executed directly through the FX market.  *  Trades executed as a result of automatic IB conversions, which occur when you trade a product in a non-base currency.  *  Trades deliberately executed to close non-base currency positions using the FXCONV destination.  | [optional]
**accrued_interest** | Option<**i32**> | Accrued interest is the interest accruing on a security since the previous coupon date. If a security is sold between two payment dates, the buyer usually compensates the seller for the interest accrued, either within the price or as a separate payment. | [optional]
**reg_t_margin** | Option<**i32**> | The initial margin requirements calculated under US Regulation T rules for both the securities and commodities segment of your account. | [optional]
**initial_margin** | Option<**i32**> | The available initial margin for the account. | [optional]
**maintenance_margin** | Option<**i32**> | The available maintenance margin for the account. | [optional]
**cash_balances** | Option<[**Vec<models::AccountSummaryResponseCashBalancesInner>**](accountSummaryResponse_cashBalances_inner.md)> | An array containing balance information for all currencies held by the account. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
