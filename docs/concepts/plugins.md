# Plugins

The `nautilus-plugin` crate defines the plug-in artifact contract for NautilusTrader. It lets an
independently compiled Rust `cdylib` identify itself with versioned build metadata and a manifest,
and provides the C-ABI boundary primitives used at that boundary. It covers artifact identity and
the boundary types only; it does not load, register, or run plug-ins.

:::warning
The plug-in ABI is early alpha and the contract is unstable. Pin plug-in builds to the matching
`nautilus-plugin` version.
:::

A plug-in is a Rust `cdylib` that exports a single `nautilus_plugin_init` entry symbol. The
`nautilus_plugin!` macro generates that symbol and the static manifest that carries the build
identity:

```rust
nautilus_plugin::nautilus_plugin! {
    name: "example-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
}
```

Set `crate-type = ["cdylib"]` in the artifact's `Cargo.toml` and depend on the matching
`nautilus-plugin` version. See [the crate docs](https://docs.rs/nautilus-plugin) for the boundary
and manifest types.
