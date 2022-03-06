# class TestCommandStubs:
#     @staticmethod
#     def cancel_order_command():
#         return CancelOrder(
#             trader_id=TestIdentityStubs.trader_id(),
#             strategy_id=TestIdentityStubs.strategy_id(),
#             instrument_id=BetfairTestIdentityStubs.instrument_id(),
#             client_order_id=ClientOrderId("O-20210410-022422-001-001-1"),
#             venue_order_id=VenueOrderId("229597791245"),
#             command_id=BetfairTestIdentityStubs.uuid(),
#             ts_init=BetfairTestComponentStubs.clock().timestamp_ns(),
#         )
