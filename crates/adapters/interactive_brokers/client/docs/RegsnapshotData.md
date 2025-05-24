# RegsnapshotData

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**conid** | Option<**i32**> | IB contract ID. | [optional]
**conid_ex** | Option<**String**> | Contract ID and routing destination in format 123456@EXCHANGE. | [optional]
**size_min_tick** | Option<**f64**> | Internal use. Minimum size display increment. | [optional]
**bbo_exchange** | Option<**String**> | Internal use. Exchange map code. | [optional]
**has_delayed** | Option<**bool**> | Indicates whether delayed data is available. | [optional]
**param_84** | Option<**String**> | Bid price. | [optional]
**param_86** | Option<**String**> | Ask price. | [optional]
**param_88** | Option<**i32**> | Bid size in round lots (100 shares). | [optional]
**param_85** | Option<**i32**> | Ask size in round lots (100 shares). | [optional]
**best_bid_exch** | Option<**i32**> | Internal use. Equivalent binary encoding of field 7068. | [optional]
**best_ask_exch** | Option<**i32**> | Internal use. Equivalent binary encoding of field 7057. | [optional]
**param_31** | Option<**String**> | Last traded price. | [optional]
**param_7059** | Option<**String**> | Last traded size in round lots (100 shares). | [optional]
**last_exch** | Option<**i32**> | Internal use. Equivalent binary encoding of field 7058. | [optional]
**param_7057** | Option<**String**> | Best ask exchanges(s). String of single, capital-letter MCOIDs. | [optional]
**param_7068** | Option<**String**> | Best bid exchange(s). String of single, capital-letter MCOIDs. | [optional]
**param_7058** | Option<**String**> | Exchange of last trade. A single, capital-letter MCOID. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
