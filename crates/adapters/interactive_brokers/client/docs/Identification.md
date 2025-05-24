# Identification

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**citizenship** | Option<**String**> | Citizenship of the applicant.<br>If citizenship, citizenship2, OR citizenship3 is classified as a ‘Prohibited Country', THEN ProhibitedCountryQuestionnaire is required.<br>List of Prohibited Countries an be obtained using /getEnumerations<br>Preferred id document by IssuingCountry | [optional]
**citizenship2** | Option<**String**> | If the applicant has multiple citizenship, provide the additional citizenship of the applicant.<br>If citizenship, citizenship2, OR citizenship3 is classified as a ‘Prohibited Country', THEN ProhibitedCountryQuestionnaire is required.<br>List of Prohibited Countries an be obtained using /getEnumerations<br>Preferred id document by IssuingCountry | [optional]
**citizenship3** | Option<**String**> | If the applicant has multiple citizenship, provide the additional citizenship of the applicant.<br>If citizenship, citizenship2, OR citizenship3 is classified as a ‘Prohibited Country', THEN ProhibitedCountryQuestionnaire is required.<br>List of Prohibited Countries an be obtained using /getEnumerations<br>Preferred id document by IssuingCountry | [optional]
**ssn** | Option<**String**> | Social security number, required for US Residents and citizens. | [optional]
**sin** | Option<**String**> | Social insurance number, required for Canada Residents and citizens. | [optional]
**drivers_license** | Option<**String**> | Drivers License<br>Pattern for AUS: ^.{0,64}$<br>Pattern for NZL: ^[A-Z]{2}\\d{6}$ | [optional]
**passport** | Option<**String**> | Passport | [optional]
**alien_card** | Option<**String**> | Alien Card | [optional]
**hk_travel_permit** | Option<**String**> | HK and Macao Travel Permit | [optional]
**medicare_card** | Option<**String**> | Only applicable for Australia residents. | [optional]
**card_color** | Option<**String**> | Required if MedicareCard is provided. | [optional]
**medicare_reference** | Option<**String**> | Required if MedicareCard is provided. | [optional]
**national_card** | Option<**String**> | National Identification Card<br>Pattern by Country-<br> ARG: ^\\d{8}$<br>AUS: ^(\\d{8}|\\d{9})$<br>BRA: ^\\d{11}$<br>CHN: ^\\d{17}(\\d|X)$<br>DNK: ^\\d{10}$<br>ESP: ^\\d{8}[A-Z]$<br>FRA: ^\\d{15}$<br>FRA: ^\\d{4}([A-Z]|\\d){3}\\d{5}$<br>ITA: ^([A-Z]{2}\\d{7}|\\d{7}[A-Z]{2}|[A-Z]{2}\\d{5}[A-Z]{2})$<br>ITA: ^[A-Z]{6}\\d{2}[A-Z]\\d{2}[A-Z]\\d{3}[A-Z]$<br>MEX: ^[A-Z]{4}\\d{6}[A-Z]{6}\\d{2}$<br>MYZ: ^\\d{12}$<br>RUS: ^\\d{10}$<br>RUS: ^\\d{9}$<br>SGP: ^[A-Z]\\d{7}[A-Z]$<br>SWE: ^(\\d{10}|\\d{12})$<br>TUR: ^\\d{11}$<br>ZAF: ^\\d{13}$ | [optional]
**issuing_country** | Option<**String**> | Issuing country of the ID document. | [optional]
**issuing_state** | Option<**String**> | Issuing state of the ID document. | [optional]
**rta** | Option<**String**> | Only applicable IF ID_Type=DriversLicense AND IssuingCountry=AUS | [optional]
**legal_residence_country** | Option<**String**> |  | [optional]
**legal_residence_state** | Option<**String**> |  | [optional]
**educational_qualification** | Option<**String**> |  | [optional]
**fathers_name** | Option<**String**> |  | [optional]
**green_card** | Option<**bool**> |  | [optional]
**pan_number** | Option<**String**> | India PanCard, required for India Residents and citizens. | [optional]
**tax_id** | Option<**String**> | Tax ID TIN within <TaxResidencies>foreign_tax_id within <W8Ben> | [optional]
**proof_of_age_card** | Option<**String**> |  | [optional]
**expire** | Option<**bool**> | Indicate IF ID document has an ExpirationDate. | [optional]
**expiration_date** | Option<[**String**](string.md)> | Provide expiration date of the ID document. Cannot be past date. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
