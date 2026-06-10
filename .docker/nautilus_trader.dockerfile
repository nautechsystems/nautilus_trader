FROM rust:1.96.0-slim-bookworm@sha256:b5f842fac1e3b4ff718a652a8e0173b62d9403ec826ef4998880b9347db30684 AS rust-toolchain

# Pin to specific digest for supply-chain security (python:3.13-slim as of 2026-04-30)
FROM python@sha256:a0779d7c12fc20be6ec6b4ddc901a4fd7657b8a6bc9def9d3fde89ed5efe0a3d AS base
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=off \
    PIP_DISABLE_PIP_VERSION_CHECK=on \
    PIP_DEFAULT_TIMEOUT=100 \
    PYO3_PYTHON="/usr/local/bin/python3" \
    PYSETUP_PATH="/opt/pysetup" \
    RUSTUP_TOOLCHAIN="1.96.0" \
    CARGO_HOME="/usr/local/cargo" \
    RUSTUP_HOME="/usr/local/rustup" \
    BUILD_MODE="release" \
    CC="clang"
ENV PATH="/root/.local/bin:/usr/local/cargo/bin:$PATH"
WORKDIR $PYSETUP_PATH

FROM base AS builder

# Install build deps
RUN apt-get update && \
    apt-get install -y curl clang lld git make pkg-config capnproto libcapnp-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install Rust
COPY --from=rust-toolchain /usr/local/cargo /usr/local/cargo
COPY --from=rust-toolchain /usr/local/rustup /usr/local/rustup

# Install UV
COPY --from=ghcr.io/astral-sh/uv:0.11.18@sha256:78bc42400d77b0678ba95765305c826652ed5431f399257271dda681d0318f03 \
  /uv /uvx /root/.local/bin/

# Install package requirements
COPY uv.lock pyproject.toml build.py ./
RUN uv sync --no-install-package nautilus_trader

# Build nautilus_trader
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY crates ./crates
RUN cargo build --lib --release --all-features

COPY nautilus_trader ./nautilus_trader
COPY README.md ./
RUN uv build --wheel
RUN uv pip install --system dist/*.whl
RUN find /usr/local/lib/python3.13/site-packages -name "*.pyc" -exec rm -f {} \;

# Final application image
FROM base AS application

COPY --from=builder /usr/local/lib/python3.13/site-packages /usr/local/lib/python3.13/site-packages
COPY --from=builder /usr/local/bin/ /usr/local/bin/
