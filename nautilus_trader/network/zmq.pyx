import zmq.asyncio


# Only one zmq context must exist per process.
cdef object context = zmq.asyncio.Context()
