# Requests

An `Actor` or `Strategy` can request custom data from a `DataClient` by sending a `DataRequest`. If the client that receives the 
`DataRequest` implements a handler for the request, data will be returned to the `Strategy`

### Example

An example of this is a `DataRequest` for an `Instrument`, which the Actor class implements (copied below). Any Actor or
Strategy can call a `request_instrument` method with an `InstrumentId` to request the instrument from a `DataClient`.

In this particular case, the Actor implements a separate method, `request_instrument`, but a similar type of 
`DataRequest` could be instantiated and called from anywhere and anytime in the Actor/Strategy code.

On the Actor/Strategy:

```python
# nautilus_trader/common/actor.pyx

cpdef void request_instrument(self, InstrumentId instrument_id, ClientId client_id=None) except *:
    """
    Request an instrument for the given parameters.
    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the request.
    client_id : ClientId, optional
        The specific client ID for the command.
        If ``None`` then will be inferred from the venue in the instrument ID.
    """
    Condition.not_none(instrument_id, "instrument_id")

    cdef DataRequest request = DataRequest(
        client_id=client_id,
        venue=instrument_id.venue,
        data_type=DataType(Instrument, metadata={
            "instrument_id": instrument_id,
        }),
        callback=self._handle_instrument_response,
        request_id=self._uuid_factory.generate(),
        ts_init=self._clock.timestamp_ns(),
    )

    self._send_data_req(request)

```


On the execution client:  

```python
# nautilus_trader/adapters/binance/spot/data.py

def request_instrument(self, instrument_id: InstrumentId, correlation_id: UUID4):
    instrument: Optional[Instrument] = self._instrument_provider.find(instrument_id)
    if instrument is None:
        self._log.error(f"Cannot find instrument for {instrument_id}.")
        return

    data_type = DataType(
        type=Instrument,
        metadata={"instrument_id": instrument_id},
    )

    self._handle_data_response(
        data_type=data_type,
        data=[instrument],  # Data engine handles lists of instruments
        correlation_id=correlation_id,
    )

```