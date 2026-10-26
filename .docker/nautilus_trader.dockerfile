FROM public.ecr.aws/docker/library/rust:1.99.1-slim-bookworm@sha256:86234e857a413004348b471d11a4ebe1aa92d2eae2b6b143cac68fd1ac3c48be AS rust-toolchain

# Pin to specific digest for supply-chain security (python:3.13-slim as of 2026-08-23).
# Keep the version tag: scripts/ci/check-docker-toolchain-pins.bash treats it as the
# canonical Docker Python version and aligns the site-packages paths below to it.
FROM public.ecr.aws/docker/library/python:3.13-slim@sha256:ffb752e139c0a19692a43af8d8523b274222dd68eebad5d583b45c2201c6e30a AS base
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
COPY --from=ghcr.io/astral-sh/uv:0.13.5@sha256:32a7bb39b496babbd997b0a39a4d66c126bdaf5661b302bf6f06abda8deca6ff \
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
RUN cd python && uv run --no-sync maturin build --locked --release --out ../dist
RUN uv pip install --system dist/*.whl
RUN find /usr/local/lib/python3.13/site-packages -name "*.pyc" -exec rm -f {} \;

# Final application image
FROM base AS application

COPY --from=builder /usr/local/lib/python3.13/site-packages /usr/local/lib/python3.13/site-packages
COPY --from=builder /usr/local/bin/ /usr/local/bin/
