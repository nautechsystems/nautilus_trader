# GetStatementsResponseData

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**data_type** | Option<**String**> | the data type of the value after decoding | [optional]
**encoding** | Option<**String**> | encoding used for the value | [optional]
**value** | Option<**String**> | Base 64 encoded String of byte[]. Byte[] represents compressed data when gzip is true | [optional]
**mime_type** | Option<**String**> | mimeType of document after decoding and serializing the value | [optional]
**gzip** | Option<**bool**> | content encoding flag. Represents whether the response is compressed | [optional]
**accept** | Option<**String**> | specify response media types that are acceptable | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
