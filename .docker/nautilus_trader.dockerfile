FROM public.ecr.aws/docker/library/rust:1.97.1-slim-bookworm@sha256:99e09cb2284e2ddbb73a995deee3e91783fd04d177602ccf6eab326d778ee777 AS rust-toolchain

# Pin to specific digest for supply-chain security (python:3.13-slim as of 2026-04-30).
# Keep the version tag: scripts/ci/check-docker-toolchain-pins.bash treats it as the
# canonical Docker Python version and aligns the site-packages paths below to it.
FROM public.ecr.aws/docker/library/python:3.13-slim@sha256:a0779d7c12fc20be6ec6b4ddc901a4fd7657b8a6bc9def9d3fde89ed5efe0a3d AS base
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=off \
    PIP_DISABLE_PIP_VERSION_CHECK=on \
    PIP_DEFAULT_TIMEOUT=100 \
    PYO3_PYTHON="/usr/local/bin/python3" \
    PYSETUP_PATH="/opt/pysetup" \
    CARGO_HOME="/usr/local/cargo" \
    RUSTUP_HOME="/usr/local/rustup" \
    CC="clang"
ENV PATH="/root/.local/bin:/usr/local/cargo/bin:$PATH"
WORKDIR $PYSETUP_PATH

FROM base AS builder

# Install build deps
RUN apt-get update && \
    apt-get install -y curl clang lld git make pkg-config capnproto libcapnp-dev patchelf && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install Rust
COPY --from=rust-toolchain /usr/local/cargo /usr/local/cargo
COPY --from=rust-toolchain /usr/local/rustup /usr/local/rustup

# Install UV
COPY --from=ghcr.io/astral-sh/uv:0.12.0@sha256:606e70c71c852d03f611b1e56a195d08648507018a7057fab82c4974c4eae105 \
  /uv /uvx /root/.local/bin/

COPY Cargo.toml ./
COPY Cargo.lock ./
COPY crates ./crates
COPY patches ./patches
COPY examples/tutorials ./examples/tutorials
COPY README.md ./
COPY python/pyproject.toml python/uv.lock ./python/
RUN cd python && uv sync --frozen --no-install-package nautilus-trader

COPY python/nautilus_trader ./python/nautilus_trader
ARG CARGO_BUILD_JOBS=2
RUN cd python && uv run --no-sync maturin build --release --out ../dist
RUN uv pip install --system dist/*.whl
RUN find /usr/local/lib/python3.13/site-packages -name "*.pyc" -exec rm -f {} \;

# Final application image
FROM base AS application

COPY --from=builder /usr/local/lib/python3.13/site-packages /usr/local/lib/python3.13/site-packages
COPY --from=builder /usr/local/bin/ /usr/local/bin/
