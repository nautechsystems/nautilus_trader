# pyo3-stub-gen 

[![DeepWiki](https://img.shields.io/badge/DeepWiki-Jij--Inc%2Fpyo3--stub--gen-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/Jij-Inc/pyo3-stub-gen)

Python stub file (`*.pyi`) generator for [PyO3] with [maturin] projects.

[PyO3]: https://github.com/PyO3/pyo3
[maturin]: https://github.com/PyO3/maturin

| crate name | crates.io | docs.rs | doc (main) |
| --- | --- | --- | --- |
| [pyo3-stub-gen] | [![crate](https://img.shields.io/crates/v/pyo3-stub-gen.svg)](https://crates.io/crates/pyo3-stub-gen)  | [![docs.rs](https://docs.rs/pyo3-stub-gen/badge.svg)](https://docs.rs/pyo3-stub-gen) | [![doc (main)](https://img.shields.io/badge/doc-main-blue?logo=github)](https://jij-inc.github.io/pyo3-stub-gen/pyo3_stub_gen/index.html) |
| [pyo3-stub-gen-derive] | [![crate](https://img.shields.io/crates/v/pyo3-stub-gen-derive.svg)](https://crates.io/crates/pyo3-stub-gen-derive)  | [![docs.rs](https://docs.rs/pyo3-stub-gen-derive/badge.svg)](https://docs.rs/pyo3-stub-gen-derive) | [![doc (main)](https://img.shields.io/badge/doc-main-blue?logo=github)](https://jij-inc.github.io/pyo3-stub-gen/pyo3_stub_gen_derive/index.html) |

[pyo3-stub-gen]: ./pyo3-stub-gen/
[pyo3-stub-gen-derive]: ./pyo3-stub-gen-derive/

> [!NOTE]
> Minimum supported Python version is 3.10. Do not enable 3.9 or older in PyO3 setting.

> [!NOTE]
> Versions 0.15.0–0.17.1 unintentionally included a LGPL dependency. This was removed in 0.17.2, and the affected versions have been yanked.

# Design
Our goal is to create a stub file `*.pyi` from Rust code, however,
automated complete translation is impossible due to the difference between Rust and Python type systems and the limitation of proc-macro.
We take semi-automated approach:

- Provide a default translator which will work **most** cases, not **all** cases
- Also provide a manual way to specify the translation.

If the default translator does not work, users can specify the translation manually,
and these manual translations can be integrated with what the default translator generates.
So the users can use the default translator as much as possible and only specify the translation for the edge cases.

[pyo3-stub-gen] crate provides the manual way to specify the translation,
and [pyo3-stub-gen-derive] crate provides the default translator as proc-macro based on the mechanism of [pyo3-stub-gen].

# Usage

If you are looking for a working example, please see the [examples](./examples/) directory.

| Example          | Description |
|:-----------------|:------------|
| [examples/pure]  | Example for [Pure Rust maturin project](https://www.maturin.rs/project_layout#pure-rust-project) |
| [examples/mixed] | Example for [Mixed Rust/Python maturin project](https://www.maturin.rs/project_layout#mixed-rustpython-project) with submodule |

[examples/pure]: ./examples/pure/
[examples/mixed]: ./examples/mixed/

Here we describe basic usage of [pyo3-stub-gen] crate based on [examples/pure] example.

## Annotate Rust code with proc-macro

This crate provides a procedural macro `#[gen_stub_pyfunction]` and others to generate a Python stub file.
It is used with PyO3's `#[pyfunction]` macro. Let's consider a simple example PyO3 project:

```rust
use pyo3::prelude::*;

#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
    Ok((a + b).to_string())
}

#[pymodule]
fn your_module_name(m: &Bound<PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    Ok(())
}
```

To generate a stub file for this project, please modify it as follows:

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::{derive::gen_stub_pyfunction, define_stub_info_gatherer};

#[gen_stub_pyfunction]  // Proc-macro attribute to register a function to stub file generator.
#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
    Ok((a + b).to_string())
}

#[pymodule]
fn your_module_name(m: &Bound<PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    Ok(())
}

// Define a function to gather stub information.
define_stub_info_gatherer!(stub_info);
```

> [!NOTE]
> The `#[gen_stub_pyfunction]` macro must be placed before `#[pyfunction]` macro.

### `#[gen_stub(skip)]`

For functions or methods that you want to exclude from the generated stub file, use the `#[gen_stub(skip)]` attribute:

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

#[gen_stub_pyclass]
#[pyclass]
struct MyClass;

#[gen_stub_pymethods]
#[pymethods]
impl MyClass {
    #[gen_stub(skip)]
    fn internal_method(&self) {
        // This method will not appear in the .pyi file
    }
}
```

### `#[gen_stub(default=xx)]`

For getters, setters, and class attributes, you can specify default values that will appear in the stub file:

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

#[gen_stub_pyclass]
#[pyclass]
struct Config {
    #[pyo3(get, set)]
    #[gen_stub(default = Config::default().timeout)]
    timeout: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config { timeout: 30 }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl Config {
    #[getter]
    #[gen_stub(default = Config::default().timeout)]
    fn get_timeout(&self) -> usize {
        self.timeout
    }
}
```

## Generate a stub file

And then, create an executable target in [`src/bin/stub_gen.rs`](./examples/pure/src/bin/stub_gen.rs) to generate a stub file:

```rust:ignore
use pyo3_stub_gen::Result;

fn main() -> Result<()> {
    // `stub_info` is a function defined by `define_stub_info_gatherer!` macro.
    let stub = pure::stub_info()?;
    stub.generate()?;
    Ok(())
}
```

and add `rlib` in addition to `cdylib` in `[lib]` section of `Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

This target generates a stub file [`pure.pyi`](./examples/pure/pure.pyi) when executed.

```shell
cargo run --bin stub_gen
```

The stub file is automatically found by `maturin`, and it is included in the wheel package. See also the [maturin document](https://www.maturin.rs/project_layout#adding-python-type-information) for more details.

## Manual Overriding

When the automatic Rust-to-Python type translation doesn't produce the desired result, you can manually specify type information using Python stub syntax. There are two main approaches:

1. **Complete override** - Replace entire function signature with `#[gen_stub_pyfunction(python = "...")]`
2. **Partial override** - Override specific arguments or return types with `#[gen_stub(override_type(...))]`

### Method 1: Complete Override Using `python` Parameter

Use the `python` parameter to specify the complete function signature in Python stub syntax. This is ideal when you need to define complex types or when the entire signature needs custom definition.

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

#[gen_stub_pyfunction(python = r#"
    import collections.abc
    import typing

    def fn_with_callback(callback: collections.abc.Callable[[str], typing.Any]) -> collections.abc.Callable[[str], typing.Any]:
        """Example using python parameter for complete override."""
"#)]
#[pyfunction]
pub fn fn_with_callback<'a>(callback: Bound<'a, PyAny>) -> PyResult<Bound<'a, PyAny>> {
    callback.call1(("Hello!",))?;
    Ok(callback)
}
```

This approach:
- ✅ Provides complete control over the generated stub
- ✅ Supports complex types like `collections.abc.Callable`
- ✅ Allows adding custom docstrings
- ✅ Import statements are automatically extracted

### Method 2: Partial Override Using Attributes

For selective overrides, use `#[gen_stub(override_type(...))]` on specific arguments or `#[gen_stub(override_return_type(...))]` on the function. This is useful when most types translate correctly but a few need adjustment.

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

#[gen_stub_pyfunction]
#[pyfunction]
#[gen_stub(override_return_type(type_repr="collections.abc.Callable[[str], typing.Any]", imports=("collections.abc", "typing")))]
pub fn get_callback<'a>(
    #[gen_stub(override_type(type_repr="collections.abc.Callable[[str], typing.Any]", imports=("collections.abc", "typing")))]
    cb: Bound<'a, PyAny>,
) -> PyResult<Bound<'a, PyAny>> {
    Ok(cb)
}
```

This approach:
- ✅ Fine-grained control over individual types
- ✅ Preserves automatic generation for other parameters
- ✅ Explicit about which types need manual specification

### Method 3: Separate Definitions Using Macros

**How `submit!` works:**

The `#[gen_stub_pyfunction]` and `#[gen_stub_pyclass]` macros automatically generate `submit!` blocks internally to register type information. You can also manually add `submit!` blocks to supplement or override this automatic registration.

When multiple type signatures exist for the same function or method, the stub generator automatically generates `@overload` decorators in the `.pyi` file. This enables proper type checking for functions that accept multiple type signatures.

**Two approaches for overloads:**

1. **`python_overload` parameter**: Define overloads inline with the function
2. **`submit!` blocks**: Keep stub definitions separate - useful for proc-macro/code generation

**Function overloads:**

Use the `python_overload` parameter to define multiple type signatures inline:

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

// Define overloads inline with python_overload parameter
#[gen_stub_pyfunction(
    python_overload = r#"
    @overload
    def process(x: int) -> int:
        """Process integer input"""
    "#
)]
#[pyfunction]
pub fn process(x: f64) -> f64 {
    x + 1.0
}
```

Generated stub:
```python
@overload
def process(x: int) -> int:
    """Process integer input"""

@overload
def process(x: float) -> float: ...  # Auto-generated from Rust
```

**Suppress auto-generation** with `no_default_overload = true`:

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

#[gen_stub_pyfunction(
    python_overload = r#"
    @overload
    def func(x: int) -> int: ...
    @overload
    def func(x: str) -> str: ...
    "#,
    no_default_overload = true  // Don't generate from Rust signature
)]
#[pyfunction]
pub fn func(ob: Bound<PyAny>) -> PyResult<PyObject> {
    // Runtime type checking
    todo!()
}
```

**Class method overloads:**

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::{derive::*, inventory::submit};

#[gen_stub_pyclass]
#[pyclass]
pub struct Calculator {}

#[gen_stub_pymethods]
#[pymethods]
impl Calculator {
    fn add(&self, x: f64) -> f64 {
        x + 1.0
    }
}

// Alternative: Use submit! for method overloads (useful for proc-macro/code generation)
submit! {
    gen_methods_from_python! {
        r#"
        class Calculator:
            @overload
            def add(self, x: int) -> int:
                """Add integer (overload)"""
        "#
    }
}
```

Benefits:
- ✅ Inline overload definitions with `python_overload` parameter
- ✅ Automatic `@overload` decorator generation
- ✅ Deterministic ordering with index-based sorting
- ✅ `submit!` syntax available for proc-macro/code generation use cases

**Advanced class method patterns:**

For more advanced patterns, see [examples/pure/src/manual_submit.rs](./examples/pure/src/manual_submit.rs):
- **Fully manual method submission** - Submit all method signatures without `#[gen_stub_pymethods]`
- **Mixing proc-macro and manual submission** - Use `#[gen_stub(skip)]` for methods that need complex type annotations

For comprehensive documentation, see [Python Stub Syntax Support](./docs/python-stub-syntax.md#advanced-patterns).

### Advanced: Using `RustType` Marker

Within Python stub syntax, you can reference Rust types directly using the `pyo3_stub_gen.RustType["TypeName"]` marker. This leverages the `PyStubType` trait implementation of the Rust type.

```rust
use pyo3::prelude::*;
use pyo3_stub_gen::{derive::*, inventory::submit};

#[pyfunction]
pub fn sum_list(values: Vec<i32>) -> i32 {
    values.iter().sum()
}

submit! {
    gen_function_from_python! {
        r#"
        def sum_list(values: pyo3_stub_gen.RustType["Vec<i32>"]) -> pyo3_stub_gen.RustType["i32"]:
            """Sum a list of integers"""
        "#
    }
}
```

The `RustType` marker automatically expands to the appropriate Python type:
- `RustType["Vec<i32>"]` → `typing.Sequence[int]` (for arguments)
- `RustType["i32"]` → `int` (for return values)

This is particularly useful for:
- Generic types like `Vec<T>`, `HashMap<K, V>`
- Custom types that implement `PyStubType`
- Ensuring consistency between Rust and Python type mappings

### When to Use Which Method

| Scenario | Recommended Method |
|----------|-------------------|
| Complex types (e.g., `Callable`, `Protocol`) | Method 1: `python = "..."` parameter |
| Override one or two arguments | Method 2: `#[gen_stub(override_type(...))]` |
| Function overloads (`@overload`) | `python_overload = "..."` parameter |
| Reference Rust types in Python syntax | Use `RustType["..."]` marker |
| Complete function signature replacement | Method 1: `python = "..."` parameter |

For complete examples, see the [examples/pure](./examples/pure/) directory, particularly:
- `overriding.rs` - Type override examples
- `overloading.rs` - Function overload examples
- `rust_type_marker.rs` - RustType marker examples

## Type Aliases

Type aliases allow you to define semantic names for complex or frequently used types in your stub files. They improve code readability and maintainability by providing meaningful names for type combinations.

### Basic Usage

Use the `type_alias!` macro to define type aliases:

```rust
use pyo3_stub_gen::type_alias;
use std::collections::HashMap;

// Simple type alias
type_alias!("your_module", SimpleAlias = Option<usize>);

// Collection types
type_alias!("your_module", StrIntMap = HashMap<String, i32>);

// Nested option types
type_alias!("your_module", MaybeString = Option<Option<String>>);
```

### Direct Union Syntax

The `type_alias!` macro supports direct union syntax, eliminating the need for a separate `impl_stub_type!` declaration in most cases:

```rust
use pyo3_stub_gen::type_alias;

// Simple union types
type_alias!("your_module", NumberOrStringAlias = i32 | String);

// Multiple types in a union
type_alias!("your_module", TripleUnion = i32 | String | bool);

// Unions of generic types
type_alias!("your_module", GenericUnion = Option<i32> | Vec<String>);

// Complex nested unions
type_alias!("your_module", ComplexUnion = Option<Vec<i32>> | Option<Vec<String>>);
```

### Alternative: Using with `impl_stub_type!`

For reusable union types that you want to reference in multiple places, you can still use the two-step `impl_stub_type!` + `type_alias!` pattern:

```rust
use pyo3_stub_gen::{impl_stub_type, type_alias};

// Define a reusable union type
struct NumberOrString;
impl_stub_type!(NumberOrString = i32 | String);

// Use it in multiple type aliases
type_alias!("your_module", NumberOrStringAlias = NumberOrString);
type_alias!("your_module", AnotherAlias = NumberOrString);
```

This approach is useful when you need to use the same union type in multiple contexts, avoiding repetition.

### Generated Output

Type aliases are rendered in Python stub files using the `TypeAlias` annotation (Python 3.11+ compatible):

```python
from typing import TypeAlias

__all__ = [
    "MaybeDecimal",
    "NumberOrStringAlias",
    "SimpleAlias",
    "StrIntMap",
    "StructUnion",
]

MaybeDecimal: TypeAlias = typing.Optional[DecimalHolder]
NumberOrStringAlias: TypeAlias = builtins.int | builtins.str
SimpleAlias: TypeAlias = typing.Optional[builtins.int]
StrIntMap: TypeAlias = builtins.dict[builtins.str, builtins.int]
StructUnion: TypeAlias = ComparableStruct | HashableStruct
```

### Type Alias Syntax Configuration

By default, pyo3-stub-gen generates type aliases using the pre-Python 3.12 syntax with `TypeAlias`:

```python
from typing import TypeAlias

MyAlias: TypeAlias = int | str
```

For projects targeting Python 3.12 or higher, you can use the newer `type` statement syntax by adding the following to your `pyproject.toml`:

```toml
[tool.pyo3-stub-gen]
use-type-statement = true
```

This will generate:

```python
type MyAlias = int | str
```

> [!NOTE]
> When using `use-type-statement = true`, ensure your project's minimum Python version is 3.12 or higher. The `type` statement is not available in earlier Python versions.

### Python Stub Syntax for Type Aliases

For complex type aliases that require Python-specific syntax, you can use `gen_type_alias_from_python!`:

```rust
use pyo3_stub_gen::derive::gen_type_alias_from_python;

gen_type_alias_from_python!(
    "your_module",
    r#"
    import collections.abc
    from typing import TypeAlias
    CallbackType: TypeAlias = collections.abc.Callable[[str], None]
    "#
);
```

The parser accepts **both** the pre-3.12 and 3.12+ syntaxes:

```rust
use pyo3_stub_gen::derive::gen_type_alias_from_python;

// Pre-3.12 syntax
gen_type_alias_from_python!(
    "your_module",
    r#"
    from typing import TypeAlias
    CallbackType: TypeAlias = collections.abc.Callable[[str], None]
    "#
);

// Python 3.12+ syntax
gen_type_alias_from_python!(
    "your_module",
    r#"
    import collections.abc
    type OptionalCallback = collections.abc.Callable[[str], None] | None
    "#
);
```

The output format is controlled by the `use-type-statement` configuration, regardless of which syntax you use in the input.

This approach is useful for:
- Types requiring specific imports (e.g., `collections.abc.Callable`)
- Complex generic types
- Types that are difficult to express with `PyStubType`

### Benefits

- **Readability**: Provide semantic names for complex types
- **Consistency**: Ensure the same type combination is used throughout
- **Maintainability**: Update the type definition in one place
- **Integration**: Works with automatic `__all__` generation and type checkers (mypy, pyright, ruff)

### Note

Type aliases are stub-only constructs and do not exist at runtime. They are purely for static type checking and IDE support.

## Advanced: mypy.stubtest integration

[mypy stubtest](https://mypy.readthedocs.io/en/stable/stubtest.html) validates that stub files match runtime behavior. You can add it to your test suite:

```bash
uv run stubtest your_module_name --ignore-missing-stub --ignore-disjoint-bases
```

### Required flags for PyO3/maturin projects

- `--ignore-missing-stub` - Maturin creates internal native modules (`.so` files) that re-export to `__init__.py`. Stubtest looks for stubs for these internal modules, which don't exist (all types are in `__init__.pyi`). This flag prevents false positives.
- `--ignore-disjoint-bases` - PyO3 classes are disjoint bases at runtime, but pyo3-stub-gen does not generate `@typing.disjoint_base` decorators.

### Known limitation: nested submodules

**Stubtest does not work with PyO3 nested submodules.** Nested `#[pymodule]` creates runtime attributes (not importable modules), but stub files use directory structure. For projects with nested submodules, disable stubtest for those packages. See `examples/mixed/Taskfile.yml` for an example.

## API Reference Documentation

In addition to stub files, pyo3-stub-gen can generate [Sphinx](https://www.sphinx-doc.org/) API reference documentation from the same Rust type metadata. This provides rendered HTML documentation with cross-references, type links, and docstrings — without writing any Python documentation manually.

### Configuration

Add a `[tool.pyo3-stub-gen.doc-gen]` section to your `pyproject.toml`:

```toml
[tool.pyo3-stub-gen.doc-gen]
output-dir = "docs/api"
json-output = "api_reference.json"
index-title = "API Reference"
```

No changes are needed to `src/bin/stub_gen.rs` — `stub.generate()` automatically generates documentation when this section is present.

Available options:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `output-dir` | Path | `"docs/api"` | Output directory for generated files (relative to `pyproject.toml`) |
| `json-output` | String | `"api_reference.json"` | JSON data filename |
| `separate-pages` | Boolean | `true` | Generate separate `.rst` page per module |
| `index-title` | String | `"{package} API Reference"` | Title for `index.rst` |
| `intro-message` | String | *(default blurb)* | Intro text for `index.rst` (empty string to omit) |
| `contents-table` | Boolean | `false` | Show module contents summary table |

### Sphinx Setup

Add Sphinx and related packages to your dev dependencies:

```toml
[dependency-groups]
dev = ["myst-parser", "sphinx", "sphinx-rtd-theme"]
```

Create a `docs/conf.py` that loads the generated extension:

```python
import sys
from pathlib import Path

# Add the API docs directory so Sphinx can find the generated extension
sys.path.insert(0, str(Path(__file__).parent / "api"))

project = "your_project"
extensions = [
    "pyo3_stub_gen_ext",       # Generated extension — reads api_reference.json
    "sphinx.ext.intersphinx",  # Enables cross-references to external projects
]

intersphinx_mapping = {
    "python": ("https://docs.python.org/3", None),
}

html_theme = "sphinx_rtd_theme"
```

### Generated Files

Running `cargo run --bin stub_gen` produces the following in `output-dir`:

- `api_reference.json` — structured API data (JSON intermediate representation)
- `pyo3_stub_gen_ext.py` — Sphinx extension that renders the JSON into documentation
- `index.rst` — table of contents with toctree (when `separate-pages = true`)
- `<module>.rst` — one page per module (when `separate-pages = true`)

Each module `.rst` file contains a single directive that the extension expands:

```rst
pure
====

.. pyo3-api:: pure
```

### Building the Documentation

```bash
cargo run --bin stub_gen                                        # Generate API data + Sphinx files
uv run --with sphinx sphinx-build -W -b html docs docs/_build   # Build HTML
```

The first command regenerates documentation whenever your Rust code changes. The second invokes Sphinx, which uses the generated extension to build HTML.

### Sphinx Directives

The generated `pyo3_stub_gen_ext.py` extension provides two RST directives:

- `.. pyo3-api:: module_name` — render a single module's API reference
- `.. pyo3-api-package:: package_name` — render all modules in a package

### Docstrings

Rust doc comments (`/// ...`) are rendered as [MyST Markdown](https://myst-parser.readthedocs.io/) in the generated documentation. This supports cross-references (`` :class:`ClassName` ``), code blocks, admonitions, and other Sphinx/MyST features. The `myst-parser` extension is required for this.

### Working Examples

See the following directories for complete setups:
- [examples/pure/docs/](./examples/pure/docs/) — single-module project
- [examples/mixed/docs/](./examples/mixed/docs/) — multi-module project with submodules

For in-depth architecture details, see [docs/docgen-architecture.md](./docs/docgen-architecture.md).

# Contribution
To be written.

# License

© 2024 Jij Inc.

This project is licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

# Links

- [MusicalNinjas/pyo3-stubgen](https://github.com/MusicalNinjas/pyo3-stubgen)
  - Same motivation, but different approach.
  - This project creates a stub file by loading the compiled library and inspecting the `__text_signature__` attribute generated by PyO3 in Python side.
- [pybind11-stubgen](https://github.com/sizmailov/pybind11-stubgen)
  - Stub file generator for [pybind11](https://github.com/pybind/pybind11) based C++ projects.
