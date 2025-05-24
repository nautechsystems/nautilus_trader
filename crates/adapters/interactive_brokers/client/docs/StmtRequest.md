# StmtRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**account_id** | **String** | account id |
**account_ids** | Option<**Vec<String>**> | array of account id's | [optional]
**start_date** | **String** | from date |
**end_date** | **String** | to date |
**multi_account_format** | Option<**String**> | possible values are consolidate, concatenate, or customConsolidate | [optional]
**crypto_consol_if_available** | Option<**bool**> | crypto consolidate flag, If request contains any accounts with crypto segment, will turn request into Crypto Consolidated | [optional][default to false]
**mime_type** | Option<**String**> | output format | [optional]
**language** | Option<**String**> | two character ISO language code | [optional][default to en]
**gzip** | Option<**bool**> | to gzip the whole response pass true | [optional][default to false]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
