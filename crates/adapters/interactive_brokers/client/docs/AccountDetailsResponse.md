# AccountDetailsResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**error** | Option<[**models::ErrorResponse**](ErrorResponse.md)> |  | [optional]
**has_error** | Option<**bool**> |  | [optional]
**error_description** | Option<**String**> |  | [optional]
**account** | Option<[**models::AccountData**](AccountData.md)> |  | [optional]
**associated_persons** | Option<[**Vec<models::AssociatedPerson>**](AssociatedPerson.md)> |  | [optional]
**associated_entities** | Option<[**Vec<models::AssociatedEntity>**](AssociatedEntity.md)> |  | [optional]
**with_holding_statement** | Option<**std::collections::HashMap<String, String>**> |  | [optional]
**market_data** | Option<[**Vec<std::collections::HashMap<String, String>>**](std::collections::HashMap.md)> |  | [optional]
**financial_information** | Option<[**std::collections::HashMap<String, serde_json::Value>**](serde_json::Value.md)> |  | [optional]
**sources_of_wealth** | Option<[**Vec<std::collections::HashMap<String, serde_json::Value>>**](std::collections::HashMap.md)> |  | [optional]
**trade_bundles** | Option<**Vec<String>**> |  | [optional]
**individual_ira_beneficiaries** | Option<[**Vec<models::IndividualIraBene>**](IndividualIRABene.md)> |  | [optional]
**entity_ira_beneficiaries** | Option<[**Vec<models::EntityIraBene>**](EntityIRABene.md)> |  | [optional]
**decedents** | Option<[**Vec<std::collections::HashMap<String, String>>**](std::collections::HashMap.md)> |  | [optional]
**restrictions** | Option<[**Vec<models::RestrictionInfo>**](RestrictionInfo.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
