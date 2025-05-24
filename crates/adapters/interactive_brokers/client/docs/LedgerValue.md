# LedgerValue

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**acctcode** | Option<**String**> | The Account ID of the requested account. | [optional]
**cashbalance** | Option<**f64**> | The given account's cash balance in this currency. | [optional]
**cashbalancefxsegment** | Option<**f64**> | The given account's cash balance in its dedicated forex segment in this currency, if applicable. | [optional]
**commoditymarketvalue** | Option<**f64**> | Market value of the given account's commodity positions in this currency. | [optional]
**corporatebondsmarketvalue** | Option<**f64**> | Market value of the given account's corporate bond positions in this currency. | [optional]
**currency** | Option<**String**> | Three-letter name of the currency reflected by this object, or 'BASE' for the account's base currency. | [optional]
**dividends** | Option<**f64**> | The given account's receivable (not yet disbursed) dividend balance in this currency. | [optional]
**exchangerate** | Option<**i32**> | Exchange rate of this currency relative to the account's base currency. | [optional]
**funds** | Option<**f64**> | The value of the given account's mutual fund holdings in this currency. | [optional]
**futuremarketvalue** | Option<**f64**> | Market value of the given account's futures positions in this currency. | [optional]
**futureoptionmarketvalue** | Option<**f64**> | Market value of the given account's futures options positions in this currency. | [optional]
**futuresonlypnl** | Option<**f64**> | PnL of the given account's futures positions in this currency. | [optional]
**interest** | Option<**f64**> | The given account's receivable interest balance in this currency. | [optional]
**issueroptionsmarketvalue** | Option<**f64**> | Market value of the given account's issuer options positions in this currency. | [optional]
**key** | Option<**String**> | Identifies the nature of data. Always takes values 'LedgerList'. | [optional]
**moneyfunds** | Option<**f64**> | The value of the given account's money market fund holdings in this currency. | [optional]
**netliquidationvalue** | Option<**f64**> | The given account's net liquidation value of positions in this currency. | [optional]
**realizedpnl** | Option<**f64**> | The given account's realized PnL for positions in this currency. | [optional]
**secondkey** | Option<**String**> | Additional identifier of the currency reflected in this object. Always matches 'currency' field. | [optional]
**sessionid** | Option<**i32**> |  | [optional]
**settledcash** | Option<**f64**> | The given account's settled cash balance in this currency. | [optional]
**severity** | Option<**i32**> |  | [optional]
**stockmarketvalue** | Option<**f64**> | Market value of the given account's stock positions in this currency. | [optional]
**stockoptionmarketvalue** | Option<**f64**> | Market value of the given account's stock options positions in this currency. | [optional]
**tbillsmarketvalue** | Option<**f64**> | Market value of the given account's treasury bill positions in this currency. | [optional]
**tbondsmarketvalue** | Option<**f64**> | Market value of the given account's treasury bond positions in this currency. | [optional]
**timestamp** | Option<**i32**> | Timestamp of retrievable of this account ledger data. | [optional]
**unrealizedpnl** | Option<**f64**> | The given account's unrealied PnL for positions in this currency. | [optional]
**warrantsmarketvalue** | Option<**f64**> | Market value of the given account's warrant positions in this currency. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
