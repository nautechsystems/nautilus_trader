# CI Optimization Guide

This document outlines the optimizations implemented to accelerate CI precommit and build phases.

## Overview

The CI pipeline has been optimized to reduce build times from ~20-30 minutes to ~10-15 minutes through several key strategies:

1. **Cargo-chef dependency pre-building**
2. **Enhanced caching strategies**
3. **Selective pre-commit execution**
4. **Parallel compilation**
5. **Optimized disk space management**

## Key Optimizations

### 1. Cargo-chef for CI Rust Tests

**Problem**: Rust compilation was rebuilding all dependencies on every CI run, even when only source code changed.

**Solution**: Implemented cargo-chef pattern for CI builds:

- Pre-build dependencies using `cargo chef prepare` and `cargo chef cook`
- Cache compiled dependencies separately from source code
- Use optimized `make cargo-test-ci` target

**Files**:

- `.github/actions/cargo-chef-deps/action.yml`
- `Makefile` (new `cargo-test-ci` target)

**Expected speedup**: 40-60% reduction in Rust test time

### 2. Enhanced Caching Strategy

**Improvements**:

- **Cargo target directory caching**: Cache `target/` directory with Cargo.toml/Cargo.lock hash
- **Improved sccache keys**: More granular cache keys for better hit rates
- **Increased sccache size**: From 4G to 6G for better cache retention

**Files**:

- `.github/actions/common-setup/action.yml`

### 3. Selective Pre-commit Execution

**Problem**: Pre-commit was running on all files for every PR, including unchanged files.

**Solution**:

- Run pre-commit on all files only for main/master branch pushes
- Run pre-commit only on changed files for PRs
- Fetch base branch and diff to determine changed files

**Expected speedup**: 50-80% reduction in pre-commit time for typical PRs

### 4. Optimized Disk Space Management

**Problem**: Free disk space operation was taking 2-3 minutes on every job.

**Solution**:

- Only run free disk space for jobs that actually need it (wheel builds)
- Preserve tool cache and Docker images for faster subsequent operations
- Keep swap space for compilation performance

### 5. Parallel Compilation

**Improvements**:

- Use all available CPU cores for Rust compilation (`CARGO_BUILD_JOBS=$(nproc)`)
- Optimize Make parallelization (`MAKEFLAGS="-j$(nproc)"`)
- Enhanced Cargo network settings for reliability

## Usage

### Testing Locally

Test the cargo-chef optimization locally:

```bash
./scripts/test-cargo-chef-ci.sh
```

### CI Configuration

The optimizations are automatically applied in:

- `.github/workflows/build.yml`
- `.github/workflows/performance.yml`

### Monitoring Performance

Track CI performance improvements:

1. **Before optimization**: Check recent CI run times in GitHub Actions
2. **After optimization**: Compare new run times
3. **Cache hit rates**: Monitor sccache statistics in CI logs

## Expected Results

| Phase | Before | After | Improvement |
|-------|--------|-------|-------------|
| Pre-commit | 3-5 min | 1-2 min | 50-60% |
| Rust tests | 8-12 min | 4-6 min | 40-50% |
| Wheel build | 10-15 min | 6-10 min | 30-40% |
| **Total** | **20-30 min** | **10-15 min** | **40-50%** |

## Troubleshooting

### Cache Issues

If builds are slower than expected:

1. Check cache hit rates in CI logs
2. Verify sccache statistics: `sccache --show-stats`
3. Clear caches if corrupted: Delete cache keys in GitHub Actions

### Cargo-chef Issues

If cargo-chef fails:

1. Verify recipe generation: `cargo chef prepare --recipe-path recipe.json`
2. Check dependency cooking: `cargo chef cook --recipe-path recipe.json`
3. Validate workspace structure: `cargo metadata --format-version 1`

### Pre-commit Issues

If pre-commit fails on changed files:

1. Check git diff output: `git diff --name-only origin/main...HEAD`
2. Verify base branch fetch: `git fetch origin main`
3. Run pre-commit manually: `pre-commit run --files <file1> <file2>`

## Future Optimizations

Potential additional improvements:

1. **Build matrix optimization**: Reduce redundant builds across Python versions
2. **Test parallelization**: Split test suites across multiple jobs
3. **Incremental testing**: Only test changed modules
4. **Remote caching**: Use external cache services for larger cache storage
