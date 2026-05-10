# Design Principles

## Message immutability

Once a message (request, response, event, or command) is created, its fields must not be mutated.
See [Message Bus: message integrity](../concepts/message_bus.md#message-integrity) for the
ownership rules that follow from this.

The invariant protects several properties the system depends on:

- **Determinism**: Every consumer sees the same input. Behavior is easier to reason about, replay,
  and test.
- **Temporal integrity**: A message preserves what was true when the system emitted it. Events and
  commands remain factual records instead of containers of drifting state.
- **Safer concurrency**: Readers do not need coordination to protect message payloads from later
  rewrites. This removes a common source of races around shared state.
- **Easier debugging**: Logs, traces, replay tools, and dead-letter inspection remain useful
  because the message still reflects the original payload.
- **Reliable replay and simulation**: Replaying a sequence yields the same logical inputs as the
  original run. This supports backtesting, incident reconstruction, and regression testing.
- **Clear ownership boundaries**: Components treat incoming messages as input. If a component needs
  a different representation, it derives new local state or a new message explicitly.
- **Better auditability**: The system can answer what it knew, when it knew it, and what it did
  from that information.
- **More robust distribution**: Serialized messages already cross process and service boundaries as
  copies. The same ownership rule keeps the in-memory model aligned with that reality.
