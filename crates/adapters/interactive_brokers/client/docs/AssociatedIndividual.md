# AssociatedIndividual

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**name** | Option<[**models::IndividualName**](IndividualName.md)> |  | [optional]
**native_name** | Option<[**models::IndividualName**](IndividualName.md)> |  | [optional]
**birth_name** | Option<[**models::IndividualName**](IndividualName.md)> |  | [optional]
**mother_maiden_name** | Option<[**models::IndividualName**](IndividualName.md)> |  | [optional]
**date_of_birth** | Option<**String**> | Date of birth of the applicant. The applicant must be 18 years or older to open an account. <br><ul><li>If the YYY-MM-DD < 18 years error will be triggered and the account will not be created.</li><li>If YYYY-MM-DD < 21 the applicant is restricted to opening a CASH account only.</li><li>UGMA and UTMA accounts are available for minors 18 years of age or younger. An individual or entity who manages an account for a minor until that minor reaches a specific age. Available to US residents only.</li><li>This application must be opened using the front-end application which is available within the IBKR Portal.</li><li>Assets held in a single account managed by a single Custodian user.</li><li>Error will be thrown if dateOfBirth is any value other than YYYY-MM-DD.</li></ul> | [optional]
**country_of_birth** | Option<**String**> |  | [optional]
**city_of_birth** | Option<**String**> |  | [optional]
**gender** | Option<**String**> |  | [optional]
**marital_status** | Option<**String**> |  | [optional]
**num_dependents** | Option<**i32**> |  | [optional]
**residence_address** | Option<[**models::ResidenceAddress**](ResidenceAddress.md)> |  | [optional]
**mailing_address** | Option<[**models::Address**](Address.md)> |  | [optional]
**phones** | Option<[**Vec<models::PhoneInfo>**](PhoneInfo.md)> |  | [optional]
**email** | Option<**String**> |  | [optional]
**identification** | Option<[**models::Identification**](Identification.md)> |  | [optional]
**employment_type** | Option<**String**> |  | [optional]
**employment_details** | Option<[**models::EmploymentDetails**](EmploymentDetails.md)> |  | [optional]
**employee_title** | Option<**String**> |  | [optional]
**tax_residencies** | Option<[**Vec<models::TaxResidency>**](TaxResidency.md)> |  | [optional]
**w9** | Option<[**models::FormW9**](FormW9.md)> |  | [optional]
**w8_ben** | Option<[**models::FormW8Ben**](FormW8BEN.md)> |  | [optional]
**crs** | Option<[**models::FormCrs**](FormCRS.md)> |  | [optional]
**prohibited_country_questionnaire** | Option<[**models::ProhibitedCountryQuestionnaireList**](ProhibitedCountryQuestionnaireList.md)> |  | [optional]
**id** | Option<**String**> |  | [optional]
**external_id** | Option<**String**> |  | [optional]
**user_id** | Option<**String**> |  | [optional]
**same_mail_address** | Option<**bool**> |  | [optional]
**authorized_to_sign_on_behalf_of_owner** | Option<**bool**> |  | [optional]
**authorized_trader** | Option<**bool**> |  | [optional]
**us_tax_resident** | Option<**bool**> |  | [optional]
**translated** | Option<**bool**> |  | [optional]
**primary_trustee** | Option<**bool**> |  | [optional]
**ownership_percentage** | Option<**f64**> |  | [optional]
**titles** | Option<[**Vec<models::Title>**](Title.md)> |  | [optional]
**authorized_person** | Option<**bool**> |  | [optional]
**reference_username** | Option<**String**> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
