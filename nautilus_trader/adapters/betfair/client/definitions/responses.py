"""
op=connection - The ConnectionMessage sent on your connection.
op=status - The StatusMessage (returned in response to every RequestMessage)
op=mcm - The MarketChangeMessage that carries the initial image and updates to markets that you have subscribed to.
op=ocm - The OrderChangeMessage that carries the initial image and updates to orders that you have subscribed to.
"""
# import msgspec
#
#
# class cancelInstructionReport(msgspec.Struct):
#     pass
