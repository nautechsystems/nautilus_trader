# FFI Memory Contract

NautilusTrader exposes a C foreign function interface (FFI) only from `nautilus-core` and
`nautilus-model`. Both crates gate the interface behind their `ffi` Cargo feature and keep the
exported modules under `crates/core/src/ffi/` and `crates/model/src/ffi/`.

Other workspace crates use Rust APIs or PyO3 bindings. The separate `nautilus-plugin` crate defines
the public guest plug‑in ABI and does not share this memory contract.

The rules below are strict. Violating them can cause undefined behavior, including double frees,
memory leaks, and invalid pointer access.

## Panic handling

Rust panics must never unwind across an `extern "C"` function. Exported functions that can panic
must route their implementation through `nautilus_core::ffi::abort_on_panic`, which logs the panic
and aborts the process before unwinding crosses the C boundary.

## `CVec` ownership

`CVec` is a C‑compatible representation of Rust vector allocation metadata. A `CVec` created from
`Vec<T>` transfers unique ownership of the allocation to the foreign caller. It is intentionally
neither `Copy` nor `Clone` in Rust, but C can still copy its fields, so callers must enforce the
same exactly‑once ownership rule.

| Step | Owner   | Action                                                                                        |
| ---- | ------- | --------------------------------------------------------------------------------------------- |
| 1    | Rust    | Convert `Vec<T>` into `CVec`, transferring the allocation to the caller.                      |
| 2    | Foreign | Read the elements without changing `ptr`, `len`, or `cap`.                                    |
| 3    | Foreign | Call the matching type‑specific `vec_drop_*` function exactly once to release the allocation. |

Forgetting the drop leaks the allocation. Dropping the same allocation more than once can corrupt
the allocator and crash the process.

Empty `CVec` values have `len == 0` and `cap == 0`. Their pointer is an opaque sentinel and must not
be dereferenced. Rust consumers must use `CVec::into_vec`, which handles the empty case before it
inspects the pointer. Borrowing consumers must use `CVec::as_slice` for the same reason.

Both methods are unsafe because the public metadata cannot prove allocation provenance, alignment,
initialization, or exclusive ownership. Any exported function that accepts a caller‑provided
`CVec` and invokes either method must:

- Be an `unsafe extern "C" fn`.
- Document the caller obligations in a `# Safety` section.
- Validate `len`, `cap`, and null‑pointer invariants before reconstructing or borrowing data.
- Use a concrete element type that matches the original `Vec<T>` allocation.

## Type-specific drop functions

There is no generic `cvec_drop`. Reconstructing every allocation as `Vec<u8>` gives the allocator
the wrong element layout for other types. Each owned vector crossing the boundary requires a drop
function for its exact element type, such as `vec_drop_book_levels`, `vec_drop_book_orders`, or
`vec_drop_fills`.

Add the drop function beside the producer so reviews can verify the pair together. Tests must cover
the empty sentinel and any metadata checks implemented by the consumer.

## Borrowed foreign buffers

Memory allocated outside Rust must not be reconstructed as `Vec<T>`. Borrow it with
`CVec::as_slice`, and copy it with `to_vec()` when Rust needs owned storage. The foreign caller keeps
ownership and must release the original buffer with the allocator that created it.

## Opaque pointers

Model objects whose layout is not `repr(C)` cross the ABI as opaque owning pointers. The generated
header forward‑declares the type, so foreign callers handle it only through exported functions.
Constructors return an owning `*mut T` from `Box::into_raw`, pure accessors take `&T` or `&mut T`
references, and every constructor must have a matching drop function:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_new(id: InstrumentId, book_type: BookType) -> *mut OrderBook {
    Box::into_raw(Box::new(OrderBook::new(id, book_type)))
}

/// # Safety
///
/// `book` must be a live owning pointer returned by [`orderbook_new`], and must not
/// be used after this call.
///
/// # Panics
///
/// Panics if `book` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orderbook_drop(book: *mut OrderBook) {
    abort_on_panic(|| {
        assert!(!book.is_null(), "`book` was NULL");
        // SAFETY: Caller guarantees `book` was allocated by `orderbook_new`
        drop(unsafe { Box::from_raw(book) });
    });
}
```

The foreign owner must call the drop function exactly once. It must not copy the pointer, consume
both copies, or use the pointer after the drop.

## Review checklist

For each new or changed FFI export:

- Keep the implementation in `nautilus-core` or `nautilus-model`.
- Use `repr(C)` for every type whose layout crosses the boundary.
- Prevent panics from unwinding across the boundary.
- Pair each owned allocation with one type‑specific release path.
- State complete pointer and ownership obligations in `# Safety` documentation.
- Add focused tests for the ownership and validation rules.
