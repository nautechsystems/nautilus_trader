# GwApiV1ExternalCashTransfersPostRequestInstruction

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**client_instruction_id** | **f64** |  |
**account_id** | **String** |  |
**currency** | **String** |  |
**amount** | **f64** |  |
**bank_instruction_method** | **String** |  |
**sending_institution** | Option<**String**> |  | [optional]
**identifier** | Option<**String**> |  | [optional]
**special_instruction** | Option<**String**> |  | [optional]
**bank_instruction_name** | **String** |  |
**sender_institution_name** | Option<**String**> |  | [optional]
**ira_deposit_detail** | Option<[**models::DepositFundsInstructionIraDepositDetail**](DepositFundsInstruction_iraDepositDetail.md)> |  | [optional]
**recurring_instruction_detail** | Option<[**models::RecurringInstructionDetail**](RecurringInstructionDetail.md)> |  | [optional]
**date_time_to_occur** | Option<**String**> |  | [optional]
**ira_withdrawal_detail** | Option<[**models::WithdrawFundsInstructionIraWithdrawalDetail**](WithdrawFundsInstruction_iraWithdrawalDetail.md)> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
