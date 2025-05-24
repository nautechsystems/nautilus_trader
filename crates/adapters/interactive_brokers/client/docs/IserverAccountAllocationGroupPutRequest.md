# IserverAccountAllocationGroupPutRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**name** | **String** | Name used to refer to your allocation group. If prev_name is specified, this will become the new name of the group. |
**prev_name** | Option<**String**> | Can be used to rename a group. Using this field will recognize the previous name, while the \"name\" filed will mark the updated name. | [optional]
**accounts** | **Vec<String>** | An array of accounts that should be held in the allocation group and, if using a User-specified allocaiton method, the value correlated to the allocation. |
**default_method** | [**models::AllocationMethod**](allocationMethod.md) |  |

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
