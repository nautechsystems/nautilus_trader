// #[cfg(all(feature = "allocator-jemalloc", feature = "allocator-mimalloc"))]
// compile_error!("Cannot enable both jemalloc and mimalloc allocators simultaneously");

#[cfg(feature = "allocator-mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// #[cfg(all(
//     feature = "allocator-jemalloc",
//     any(target_os = "macos", target_os = "linux"),
//     not(all(target_arch = "aarch64", target_env = "musl"))
// ))]
// #[global_allocator]
// static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// // Jemallocator does not work on aarch64 with musl, fallback to system allocator
// #[cfg(all(
//     feature = "allocator-jemalloc",
//     target_arch = "aarch64",
//     target_env = "musl",
// ))]
// #[global_allocator]
// static GLOBAL: std::alloc::System = std::alloc::System;
