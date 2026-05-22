# Plugins

The plug-in system extends a Nautilus live node with independently compiled Rust cdylibs. The host
loads each cdylib at process startup and runs its actors, strategies, and custom-data types
alongside compiled-in components. The host owns the C-ABI boundary; plug-in authors write standard
Rust traits, and a macro emits the boundary glue.

**The core philosophy**:

- The boundary is C ABI, because Rust's `#[repr(Rust)]` layout is unstable across compilations.
- Authors write normal Rust traits; macros generate the `extern "C"` thunks and `#[repr(C)]` vtables.
- Plug-ins load at process startup, register through a validated manifest, and live for the process lifetime.
- The host adapts each plug-in instance into a `DataActor` or `Strategy` so the live engine sees no FFI.
- Callbacks from a plug-in back into the host route through a single static `HostVTable` of function pointers.
- Every plug-in callback runs under `catch_unwind`. A panic in a fallible plug-in thunk surfaces as
  a `PluginError`; a panic in an infallible plug-in thunk (`create`, `drop_handle`, custom-data
  `ts_event`/`ts_init`/`clone_handle`/`drop_handle`/`eq_handles`) aborts the process. Neither path
  unwinds across the FFI boundary.

:::warning
The plug-in ABI and `LiveNodeConfig` wiring are early alpha. `NAUTILUS_PLUGIN_ABI_VERSION` stays
pinned at `1` and does not promise compatibility between Nautilus versions. Pin plug-in builds to
the matching host version, and treat the concepts here as the design contract for current
development.
:::

## Terms

- Plug-in: a Rust cdylib that exports a single `nautilus_plugin_init` symbol.
- Plug point: one trait surface a plug-in can contribute to (custom data, actor, strategy).
- Manifest: a `'static PluginManifest` returned from `nautilus_plugin_init` enumerating contributions.
- VTable: a `#[repr(C)]` struct of function pointers the host calls for one plug point on one type.
- `HostVTable`: the function-pointer table the host hands every plug-in for re-entrant callbacks.
- `HostContext`: an opaque per-instance pointer that lets host thunks attribute callbacks to the calling adapter.
- Adapter: the host-side `PluginActorAdapter` or `PluginStrategyAdapter` that wraps a plug-in handle.

## What a plug-in contributes

A v1 plug-in cdylib can publish three families of contributions through its manifest:

- Custom-data types via `PluginCustomData` (`surfaces::custom_data`).
- Plug-in actors via `PluginActor` (`surfaces::actor`).
- Plug-in strategies via `PluginStrategy` (`surfaces::strategy`).

Each family has its own `#[repr(C)]` vtable struct, an author-facing trait, and a registration entry
the manifest lists in a `Slice<'static, Registration>`. Adding a future plug point means adding one
module and one slice field, then bumping the ABI version.

Each plug-point family carries a fixed callback set. The actor surface today covers the lifecycle
hooks plus the data callbacks whose payload types are `#[repr(C)]`-clean end-to-end: quotes, trades,
bars, mark/index/funding prices, instrument status and close, order filled and canceled events,
signals, and time events. The strategy surface adds the order lifecycle and position event
callbacks on top of the actor surface.

## Boundaries

The plug-in system is intentionally narrow. Out of scope today:

- Async client adapters for data and execution.
- Catalog, cache, and event-store backends as plug-ins.
- Pre-trade risk gating as a plug-in.
- Hot reload (plug-ins load at process startup and stay loaded).
- `OrderBookDeltas`, `OrderBook`, `Instrument`, and arbitrary `CustomData` on the actor or
  strategy callback surface (payload shapes are not `#[repr(C)]`-clean).
- Backtest engine integration (plug-in loading is a live-node-only path).

## ABI boundary

Only `#[repr(C)]` types may cross between an independently compiled plug-in and the host. The
boundary primitives live in `nautilus_plugin::boundary`:

- `BorrowedStr<'a>`: pointer plus length into producer-owned UTF-8 storage.
- `Slice<'a, T>`: pointer plus length into producer-owned `[T]` storage.
- `OwnedBytes`: a producer-allocated byte buffer with its own drop thunk.
- `PluginError` and `PluginResult<T>`: tagged error envelope returned across the boundary.

Data-bearing callbacks cross the boundary as `*const T` pointers into the host's already-`#[repr(C)]`
model types. The host keeps the value alive for the call; the plug-in thunk dereferences once and
hands an `&T` to the trait method. No serialisation per event, no per-event allocation.

Order commands flow the other direction as opaque JSON. The plug-in posts a JSON payload to
`HostVTable::submit_order`, `cancel_order`, or `modify_order`; the host deserialises into
`SubmitOrderCommand`, `CancelOrderCommand`, or `ModifyOrderCommand` and dispatches to the matching
`Strategy` method. JSON keeps the command schema free to evolve without breaking the in-engine
`TradingCommand` shape.

## Manifest

The manifest is process-lifetime static data the plug-in returns from `nautilus_plugin_init`. It
identifies the build and enumerates every plug-point contribution:

- `abi_version`: must equal `NAUTILUS_PLUGIN_ABI_VERSION` or the host refuses to load.
- `plugin_name`, `plugin_vendor`, `plugin_version`: identifier strings.
- `build_id`: a versioned `PluginBuildId` carrying `nautilus-plugin` crate version, `rustc` version,
  target triple, and build profile.
- `custom_data`, `actors`, `strategies`: registration slices, one per plug point.

The loader runs `ValidatedPluginManifest::new` on the manifest before exposing it to the live node.
Validation checks identifier strings, the build-id schema version, every registration vtable
pointer, every required vtable slot, and uniqueness of type names across all plug points. The build
identifier itself stays diagnostic: empty `rustc_version`, `target_triple`, or `build_profile`
strings do not make a manifest invalid.

## Load flow

```mermaid
flowchart LR
    Config["LiveNodeConfig.plugins"] --> Verify["verify_plugin_sha256"]
    Verify --> Loader["PluginLoader::load"]
    Loader --> Init["plug-in nautilus_plugin_init"]
    Init --> Manifest["Validated PluginManifest"]
    Manifest --> CustomData["register_manifest_custom_data"]
    Manifest --> Entry["configured_entry by type_name"]
    Entry --> Actor["PluginActorAdapter / PluginStrategyAdapter"]
    Actor --> Engine["DataActor / Strategy registration"]
```

The operational steps are:

- The node clones the configured plug-in entries and refuses to load while it is not `Idle`.
- For each path, the node verifies the optional SHA-256 digest in
  `LiveNode::load_configured_plugins`, then asks the loader to `dlopen` the cdylib and resolve
  `nautilus_plugin_init`. `PluginLoader` itself does not hash the file.
- The plug-in's init thunk receives the host's `HostVTable` pointer and returns its static manifest.
- The loader runs structural validation. Failure produces a `LoadError` whose diagnostics include
  the plug-in name, version, and full `PluginBuildId`.
- The node walks every loaded manifest once to register custom-data deserializers with
  `nautilus_model::data::registry`.
- The node walks the configured entries again, resolves each `type_name` to either an actor or
  strategy registration, and instantiates an adapter through the plug-in's `create` thunk.
- The adapter is added to the trader, after which the live engine drives it like any
  compiled-in component.

The loader stops on the first error and leaks every successfully opened `Library` for the process
lifetime, because manifest, vtable, and `drop_fn` pointers the host has copied into its registries
must outlive the loader.

## Adapter routing

Once an adapter is registered, callbacks flow in both directions through stable function pointers:

```mermaid
flowchart LR
    Engine["Live engine event"] --> Adapter["PluginActorAdapter / PluginStrategyAdapter"]
    Adapter --> Guard["catch_unwind guard"]
    Guard --> Thunk["plug-in extern C thunk"]
    Thunk --> Trait["PluginActor / PluginStrategy method"]
    Trait --> Host["HostVTable callback"]
    Host --> Resolve["HostContextInner -> ActorId"]
    Resolve --> Live["Strategy::submit_order, cache reads, msgbus publish, timers"]
```

Forward calls (engine to plug-in) start in the adapter, which holds the plug-in's opaque handle and
its validated vtable. Two panic-guard layers sit between the engine and the plug-in code: the
adapter wraps each FFI call in its own `catch_unwind`-based `guard_call` as defence in depth, and
the macro-generated plug-in thunks wrap their bodies in `nautilus_plugin::panic::guard` (fallible
thunks, which surface a panic as `PluginError::Panic`) or `guard_infallible` (infallible thunks
such as `create`, `drop_handle`, and custom-data `ts_event`/`ts_init`/`clone_handle`/`eq_handles`,
which log and abort because there is no sentinel that would not silently corrupt downstream state).

Reverse calls (plug-in to host) flow through `HostVTable`. The host's static vtable bundles the
function pointers that route through the production cache, msgbus, clock, timer, and order
pipelines. The stateful ctx-taking thunks (cache reads, subscriptions, msgbus publishes, timers,
and order commands) recover the calling adapter from a per-instance `HostContextInner` payload the
host hands the plug-in at create time; that payload carries the `ActorId` and an `is_strategy`
flag. Order-command thunks reject calls from actor contexts, because actors must not submit
orders. The stateless slots (`clock_now_ns`, `log`) take no `HostContext` and do not resolve an
adapter.

The default `HostVTable` shipped with `nautilus-plugin` returns `NotImplemented` for stateful
callbacks. The live node installs a populated vtable via `plugin_loader()` so plug-ins running
inside the node reach the real cache, risk, and execution paths.

## Lifecycle

A plug-in instance follows the same lifecycle as a compiled-in actor or strategy:

```mermaid
flowchart TD
    Load["Library opened, manifest validated"] --> Create["create thunk constructs handle"]
    Create --> Register["Adapter added to trader"]
    Register --> Start["on_start called by engine"]
    Start --> Run["Lifecycle and data callbacks"]
    Run --> Stop["on_stop called by engine"]
    Stop --> Dispose["on_dispose, drop_handle"]
    Dispose --> Process["Library remains loaded until process exit"]
```

Key points:

- `create` runs once per configured instance. The adapter passes the plug-in its `HostVTable`
  pointer, its `HostContextInner` pointer, and the verbatim JSON config payload.
- Adapter drop runs the plug-in's `drop_handle` thunk and releases the heap-allocated
  `HostContextInner` allocation.
- `dlclose` is intentionally never called in v1. The `LoadedPlugin` wraps its `libloading::Library`
  in `ManuallyDrop` so manifest and vtable pointers copied into the host's registries never dangle.

## Configuration

Plug-in instances are declared on `LiveNodeConfig.plugins` as a list of `PluginConfig` entries:

```toml
[[plugins]]
path = "./target/debug/examples/libcustom_data_plugin.so"
type_name = "ExampleStrategy"
sha256 = "<optional 64-char hex digest>"

[plugins.config]
strategy_id = "STRAT-001"
order_id_tag = "001"
threshold = 10
```

Each entry binds one plug-in instance:

- `path`: absolute or working-directory-relative path to the cdylib. Repeated paths are loaded
  once and shared across entries.
- `type_name`: the canonical type name from the plug-in manifest. The host rejects the entry if
  the manifest exposes the name as both an actor and a strategy.
- `sha256`: optional lowercase hex SHA-256 digest of the cdylib. If set, the node hashes the file
  before loading and aborts on mismatch.
- `config`: a free-form JSON object serialised verbatim into the `config_json` argument the
  plug-in's `create` thunk receives.

The node interprets a few well-known keys inside `config` when instantiating an entry:

- `actor_id`: identifier assigned to the adapter's `ActorId`. Defaults to the manifest `type_name`.
- `strategy_id`: identifier assigned to the adapter's `StrategyId`. Defaults to `<type_name>-001`.
- `order_id_tag`: optional order ID tag forwarded into the strategy's `StrategyConfig`.
- `strategy_config`: optional fully-formed `StrategyConfig` JSON value, used for strategy plug-ins
  that need more than the three keys above.

Plug-in support is gated behind the `plugin` Cargo feature on the live crate, which is on by
default. A build compiled with `--no-default-features` (or any feature set that omits `plugin`)
rejects a non-empty `plugins` list with a clear error so plug-in users cannot accidentally run
without host-side support compiled in.

## Author API

Plug-in authors implement one trait per plug-point family and call the `nautilus_plugin!` macro:

```rust
use nautilus_model::data::QuoteTick;
use nautilus_plugin::prelude::*;

#[derive(Default)]
pub struct ExampleActor {
    quotes_seen: u64,
}

impl PluginActor for ExampleActor {
    const TYPE_NAME: &'static str = "ExampleActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self::default()
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.quotes_seen += 1;
        Ok(())
    }
}

nautilus_plugin::nautilus_plugin! {
    name: "example-actor-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
    actors: [ExampleActor],
}
```

The macro emits `nautilus_plugin_init`, the `'static PluginManifest`, and the per-plug-point
vtables. Fallible thunks forward through `panic::guard`; the heavier infallible thunks
(`create`, `drop_handle`, and custom-data `ts_event`/`ts_init`/`clone_handle`/`eq_handles`)
forward through `guard_infallible`; trivial slots that cannot panic (the `type_name` thunks, which
just return a `BorrowedStr` over a `&'static str` constant) carry no guard at all.

Authors never write `extern "C"` or `#[repr(C)]`. `unsafe` requirements depend on what the plug-in
holds. The example actor in `crates/plugin/examples/custom_data_plugin.rs` discards the
`*const HostVTable` and `*const HostContext` pointers that `PluginActor::new` receives, so it
needs no `unsafe`. Plug-ins that store those pointers (whether actor or strategy) need an
`unsafe impl Send` on the struct, and any direct call into a `HostVTable` slot is
`unsafe extern "C"` and therefore `unsafe` to invoke.

`Cargo.toml` for the cdylib needs `crate-type = ["cdylib"]` and a dependency on the matching
`nautilus-plugin` version. The artifact lands at
`target/<profile>/<libname>.<so|dylib|dll>` depending on the host platform.

Build a cdylib example shipped with the crate:

```fish
cargo build -p nautilus-plugin --example custom_data_plugin
```

## Operational use today

The current alpha is focused on running Rust-native plug-ins inside a live node:

- Pin every plug-in build to the host's `nautilus-plugin` crate version, `rustc` version, and
  target triple. The loader enforces only the `abi_version` and `build_id.schema_version` fields:
  `LoadError::AbiMismatch` fires only when `abi_version` does not match, and only the build-id
  schema version is checked during validation. The crate version, `rustc`, target triple, and
  build profile remain diagnostic; the loader logs them and includes them in load-error
  diagnostics, but two builds with matching ABI and schema versions and differing build metadata
  still load successfully.
- Treat the SHA-256 field as the deployment seam for distinguishing a vetted build from any other
  cdylib at the same path.
- Inspect loader output under the `nautilus_plugin` log target. The loader logs the resolved
  plug-in name, ABI version, build identifier, and counts of registered plug points per file.
- Expect a `LoadError` to abort startup. The node refuses to load plug-ins after leaving the
  `Idle` state, so a failed load surfaces before any client connects.
- A plug-in panic in a fallible thunk returns `PluginError::Panic` to the host; a panic in an
  infallible thunk (e.g. `create`, `drop_handle`, custom-data `ts_event`) aborts the process under
  `guard_infallible` because any other behaviour would silently corrupt downstream state.

## Relationship to compiled-in components

Plug-in actors and strategies share the runtime contract of compiled-in `DataActor` and `Strategy`
implementations:

- The trader registers `PluginActorAdapter` and `PluginStrategyAdapter` through the same APIs
  it uses for in-tree components.
- Order commands (`submit_order`, `cancel_order`, `modify_order`) route through the production
  `Strategy` methods on the adapter, so risk gates, OMS conventions, and event-store capture all
  apply. Cache reads call `adapter.cache()` directly, msgbus publishes call `msgbus::publish_any`,
  and timer callbacks drive `adapter.clock()` directly; those paths bypass the `Strategy` layer
  by design.
- The plug-in author writes plain Rust trait impls. The macro hides `extern "C"` and `#[repr(C)]`
  declarations; `unsafe` is only required when the plug-in stores the raw host pointers or calls
  `HostVTable` slots directly. In-tree component patterns transfer once the author accepts those
  boundary-only obligations.

The boundary itself is the only difference: a plug-in lives in a separate cdylib, ships its own
manifest, and gives up the compiler's `#[repr(Rust)]` flexibility in exchange for being able to
ship out-of-tree without recompiling the host.
