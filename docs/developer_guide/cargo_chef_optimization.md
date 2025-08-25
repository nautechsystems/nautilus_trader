# Cargo-Chef Docker Build Optimization

## Overview

Nautilus Trader uses [cargo-chef](https://github.com/LukeMathWalker/cargo-chef) to dramatically speed up Docker builds by optimizing Rust dependency caching. This optimization can provide **5x+ speedup** in CI builds by leveraging Docker layer caching more effectively.

## How It Works

### The Problem
Traditional Rust Docker builds suffer from poor cache utilization:

- When source code changes, Docker invalidates all subsequent layers
- This forces rebuilding of all dependencies, even unchanged ones
- Large projects with many dependencies (like Nautilus Trader with 33+ crates) suffer significant build time penalties

### The Solution: cargo-chef + sccache
We use a **two-layer caching strategy** for optimal performance:

**cargo-chef** (Layer-level caching):

1. **Recipe Generation**: Creates a JSON "recipe" containing only dependency information
2. **Dependency Pre-building**: Builds dependencies in a separate layer that only invalidates when dependencies change
3. **Source Code Isolation**: Source code changes don't affect dependency cache

**sccache** (Artifact-level caching):

1. **Fine-grained Caching**: Caches individual compilation artifacts within dependencies
2. **Cross-layer Persistence**: Reuses compiled objects even when Docker layers invalidate
3. **Incremental Rebuilds**: Only recompiles changed dependencies, not all dependencies

## Implementation Details

### Multi-Stage Docker Build

Our optimized Dockerfile uses a 4-stage build process:

```dockerfile
# Stage 1: Base image with common tools
FROM python:3.13-slim AS base

# Stage 2: Rust toolchain + cargo-chef + sccache installation
FROM base AS rust-base
RUN cargo install cargo-chef --version ^0.1 sccache --locked
ENV RUSTC_WRAPPER=sccache SCCACHE_DIR=/sccache

# Stage 3: Recipe generation (planner)
FROM rust-base AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

# Stage 4: Dependency building + application build (builder)
FROM rust-base AS builder
COPY --from=planner /opt/pysetup/recipe.json recipe.json
# Build dependencies (cached layer)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --lib --release --all-features
```

### BuildKit Cache Mounts

We also use BuildKit cache mounts for additional optimization:

```dockerfile
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/opt/pysetup/target \
    cargo chef cook --release --recipe-path recipe.json
```

This provides:

- **Registry Cache**: Avoids re-downloading crates
- **Git Cache**: Caches git dependencies
- **sccache Cache**: Persists individual compilation artifacts
- **Target Cache**: Reuses compilation artifacts across builds

## cargo-chef + sccache: Why Both?

### Complementary Caching Layers

cargo-chef and sccache work **together**, not redundantly:

| Cache Level | Tool | What It Caches | When It Helps |
|-------------|------|----------------|---------------|
| **Docker Layer** | cargo-chef | Entire dependency build | Source code changes |
| **Compilation Artifact** | sccache | Individual compiled objects | Dependency changes |

### Performance Matrix

| Scenario | cargo-chef Only | + sccache | Improvement |
|----------|----------------|-----------|-------------|
| Source code change | 5x speedup | 5x speedup | Same |
| Single dependency change | No speedup | 3x speedup | **Major** |
| Multiple dependency changes | No speedup | 2x speedup | **Significant** |
| No changes | Full speedup | Full speedup | Same |

### Why You Need Both

- **cargo-chef alone**: Great for source changes, but rebuilds ALL dependencies when any dependency changes
- **sccache alone**: Helps with incremental builds, but doesn't prevent Docker layer invalidation
- **Together**: Optimal caching at both Docker layer and compilation artifact levels

## CI Integration

### GitHub Actions Configuration

The CI workflow uses enhanced caching strategies:

```yaml
cache-from: |
  type=gha,scope=nautilus-trader-nightly
  type=gha,scope=nautilus-trader-shared
cache-to: |
  type=gha,mode=max,scope=nautilus-trader-nightly
  type=gha,mode=max,scope=nautilus-trader-shared
```

### Cache Scoping

We use different cache scopes:

- **Branch-specific**: `nautilus-trader-nightly`, `nautilus-trader-latest`
- **Shared**: `nautilus-trader-shared` for common dependencies
- **Mode=max**: Ensures maximum cache retention

## Performance Benefits

### Expected Speedups

Based on cargo-chef benchmarks and our project characteristics:

| Scenario | Before | After | Speedup |
|----------|--------|-------|---------|
| Clean build | ~15-20 min | ~15-20 min | 1x (first time) |
| Source code change | ~10-15 min | ~2-3 min | **5x** |
| Dependency change | ~10-15 min | ~5-8 min | **2x** |
| No changes | ~30 sec | ~30 sec | 1x (already cached) |

### Cache Hit Scenarios

- **Recipe unchanged**: Dependencies are fully cached, only source compilation needed
- **Minor dependency changes**: Only affected dependencies rebuild
- **Major dependency changes**: Most dependencies still cached

## Local Development

### ⚠️ CRITICAL SAFETY WARNING

**NEVER run cargo-chef commands directly in your source directory!**

cargo-chef creates dummy source files that will **overwrite your actual source code**. Always use cargo-chef only within Docker containers.

### Building Locally

To build with cargo-chef optimization locally:

```bash
# Build the Docker image
docker build -f .docker/nautilus_trader.dockerfile -t nautilus-trader .

# Use BuildKit for cache mounts (recommended)
DOCKER_BUILDKIT=1 docker build -f .docker/nautilus_trader.dockerfile -t nautilus-trader .
```

### Testing the Optimization

1. **First build** (establishes cache):

   ```bash
   time docker build -f .docker/nautilus_trader.dockerfile -t nautilus-test .
   ```

2. **Make a source code change** and rebuild:

   ```bash
   # Edit a .rs file in nautilus_trader/
   time docker build -f .docker/nautilus_trader.dockerfile -t nautilus-test .
   ```

3. **Compare build times** - the second build should be significantly faster.

## Troubleshooting

### Common Issues

1. **Cache not working**: Ensure BuildKit is enabled

   ```bash
   export DOCKER_BUILDKIT=1
   ```

2. **cargo-chef installation fails**: Check Rust toolchain version compatibility

3. **Recipe generation errors**: Verify all Cargo.toml files are valid

### Debugging

Enable verbose output to see cache hits/misses:

```bash
docker build --progress=plain -f .docker/nautilus_trader.dockerfile .
```

## Maintenance

### Updating cargo-chef

The Dockerfile pins cargo-chef to `^0.1` for stability. To update:

1. Check latest version: <https://crates.io/crates/cargo-chef>
2. Update version in Dockerfile
3. Test build process
4. Update documentation if needed

### Monitoring Performance

Track build times in CI to ensure optimization remains effective:

- Monitor GitHub Actions build duration
- Compare before/after metrics
- Alert on regression

## References

- [cargo-chef GitHub Repository](https://github.com/LukeMathWalker/cargo-chef)
- [Fast Rust Docker Builds Blog Post](https://lpalmieri.com/posts/fast-rust-docker-builds/)
- [Docker BuildKit Documentation](https://docs.docker.com/build/buildkit/)
- [GitHub Actions Cache Documentation](https://docs.github.com/en/actions/using-workflows/caching-dependencies-to-speed-up-workflows)
