FROM ghcr.io/astral-sh/uv:0.7.12-python3.13-bookworm-slim AS base
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PYSETUP_PATH="/opt/pysetup" \
    RUSTUP_TOOLCHAIN="stable" \
    BUILD_MODE="release" \
    CC="clang" \
    UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy
ENV PATH="/root/.local/bin:/root/.cargo/bin:$PATH"
WORKDIR $PYSETUP_PATH

FROM base AS builder

# Install build deps
RUN apt-get update && \
    apt-get install -y curl clang git make pkg-config capnproto libcapnp-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y

# Install package requirements (dependencies only, not the project itself)
COPY uv.lock pyproject.toml build.py ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --locked --no-install-package nautilus_trader --compile-bytecode

# Set PYO3_PYTHON to point to UV-managed Python
ENV PYO3_PYTHON=$PYSETUP_PATH/.venv/bin/python

# Build nautilus_trader
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY crates ./crates
RUN cargo build --lib --release --all-features

COPY nautilus_trader ./nautilus_trader
COPY README.md ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv build --wheel
RUN --mount=type=cache,target=/root/.cache/uv \
    uv add --find-links dist nautilus_trader
RUN find $PYSETUP_PATH/.venv/lib/python3.13/site-packages -name "*.pyc" -exec rm -f {} \;

# Final application image
FROM base AS application

# Copy the entire virtual environment from builder
COPY --from=builder $PYSETUP_PATH/.venv $PYSETUP_PATH/.venv

# Update PATH to include virtual environment
ENV PATH="$PYSETUP_PATH/.venv/bin:$PATH"

# Set working directory
WORKDIR /app

# Create non-root user for security
RUN groupadd -r nautilus && useradd -r -g nautilus -m nautilus && \
    chown -R nautilus:nautilus /app
USER nautilus

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD python -c "import nautilus_trader; print('OK')" || exit 1

# Default command
CMD ["python", "-c", "import nautilus_trader; print('NautilusTrader installed successfully')"]
