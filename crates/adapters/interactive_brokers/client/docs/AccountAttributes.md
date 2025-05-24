# AccountAttributes

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**account_alias** | Option<**String**> | User-defined alias assigned to the account for easy identification. | [optional]
**account_status** | Option<**i32**> | Unix epoch timestamp of account opening. | [optional]
**account_title** | Option<**String**> | A name assigned to the account, typically the account holder name or business entity. | [optional]
**account_van** | Option<**String**> | The account's virtual account number, or otherwise its IB accountId if no VAN is set. | [optional]
**acct_cust_type** | Option<**String**> | Identifies the type of client with which the account is associated, such as an individual or LLC. | [optional]
**brokerage_access** | Option<**bool**> | Indicates whether account can receive live orders (do not mix with paper trading). | [optional]
**business_type** | Option<**String**> | A descriptor of the nature of the account, reflecting the responsible group within IB. | [optional]
**clearing_status** | Option<**String**> | Status of the account with respect to clearing at IB. O is open, P pending, N new, A abandoned, C closed, R rejected. | [optional]
**covestor** | Option<**bool**> | Indicates a Covestor account. | [optional]
**currency** | Option<**String**> | Base currency of the account. | [optional]
**desc** | Option<**String**> | Internal human-readable description of the account. | [optional]
**display_name** | Option<**String**> | Displayed name of the account in UI. Will reflect either the accountId or accountAlias, if set. | [optional]
**fa_client** | Option<**bool**> | Indicates that the account is managed by a financial advisor. | [optional]
**ib_entity** | Option<**String**> | IB business entity under which the account resides. | [optional]
**id** | Option<**String**> | The account's IB accountId. | [optional]
**no_client_trading** | Option<**bool**> | Indicates that trading by the client is disabled in the account. | [optional]
**parent** | Option<[**models::AccountAttributesParent**](accountAttributes_parent.md)> |  | [optional]
**prepaid_crypto_p** | Option<**bool**> | Indicates whether account has a prepaid crypto segment (Crypto Plus) with PAXOS. | [optional]
**prepaid_crypto_z** | Option<**bool**> | Indicates whether account has a prepaid crypto segment (Crypto Plus) with ZEROHASH. | [optional]
**track_virtual_fx_portfolio** | Option<**bool**> | Indicates that virtual forex positions are tracked in the account. | [optional]
**trading_type** | Option<**String**> | Internal identifier used by IB to reflect the trading permissions of the account. | [optional]
**r#type** | Option<**String**> | Indicates whether the account exists in production, paper, or demo environments. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
