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

## Behavioral model architecture

Use this pattern when adding or changing a model family that controls simulation or execution
behavior. It keeps the supported Rust implementations explicit and exposes supported concrete
built-in models through Python configuration.

1. Define a Rust `<Family>Model` trait as the behavioral contract.
1. Implement each built-in model as a concrete Rust type.
1. List the core built-ins and any language bridges that need enum storage in a `<Family>ModelAny`
   enum, then implement the trait with explicit enum dispatch.
1. Add a `<Family>ModelHandle` when a runtime component must share a model or accept a Rust trait
   implementation outside the enum's declared variants. Keep built-in-only storage typed as
   `<Family>ModelAny`.
1. Expose each supported concrete built-in as a PyO3 class, add its
   [type stub annotations](rust.md#type-stub-annotations), and regenerate the
   [Python artifacts](rust.md#generated-python-artifacts).
1. Accept concrete model objects directly in backtest configuration. Do not add separate model
   configuration and factory wrappers.

Adapter-specific models remain in their adapter crate when adding them to a core enum would create
a reverse dependency. Low-level Rust code can pass such a model through the corresponding handle.
Python exposure requires an explicit bridge for that model family. A bridge may be an enum variant
or pass its trait implementation through the handle, depending on the storage boundary. Latency and
margin configuration accept built-in models only from Python.

### Dispatch boundary

| Form                     | Accepted implementations               | Dispatch     | Use                                           |
| ------------------------ | -------------------------------------- | ------------ | --------------------------------------------- |
| Concrete type or generic | One concrete implementation            | Static       | Model internals and specialized callers.      |
| `<Family>ModelAny`       | Declared built-ins and bridge variants | Enum match   | Built-in and bridge configuration or storage. |
| `<Family>ModelHandle`    | Any accepted Rust trait implementation | Trait object | Shared runtime storage and custom types.      |

`<Family>ModelAny` uses enum dispatch. `<Family>ModelHandle` uses dynamic dispatch through a trait
object, so a built-in converted from the enum into a handle crosses a vtable before its enum match.
Keep a built-in-only call path typed as `<Family>ModelAny` when avoiding trait-object dispatch
matters. A handle with separate built-in and dynamic variants would be a different representation
and requires a measured performance case before adding its complexity.

### Native extension boundary

The open-source [plug-in crate](plugins.md) defines an artifact ABI, but model registration and the
loading host are not part of this repository. The open-source distribution does not provide runtime
native model plugins. Native models are composed at compile time and passed through the
corresponding enum or handle. The v1 `Importable*ModelConfig` and `MarginModelConfig` types and their
factories loaded Python or Cython classes by import path; v1 never provided native model plugins.
