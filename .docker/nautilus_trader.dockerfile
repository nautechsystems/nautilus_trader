FROM python:3.12-slim as base
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=off \
    PIP_DISABLE_PIP_VERSION_CHECK=on \
    PIP_DEFAULT_TIMEOUT=100 \
    POETRY_VERSION=1.8.1 \
    POETRY_HOME="/opt/poetry" \
    POETRY_VIRTUALENVS_CREATE=false \
    POETRY_NO_INTERACTION=1 \
    PYSETUP_PATH="/opt/pysetup" \
    BUILD_MODE="release"
ENV PATH="/root/.cargo/bin:$POETRY_HOME/bin:$PATH"
WORKDIR $PYSETUP_PATH

FROM base as builder

# Install build deps
RUN apt-get update && \
    apt-get install -y curl clang git libssl-dev make pkg-config && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install Rust stable and poetry
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y && \
    curl -sSL https://install.python-poetry.org | python3 -

# Install package requirements (split step and with --no-root to enable caching)
COPY poetry.lock pyproject.toml build.py ./
RUN poetry install --no-root --only main

# Build nautilus_trader
COPY nautilus_core ./nautilus_core
RUN (cd nautilus_core && cargo build --release)

COPY nautilus_trader ./nautilus_trader
COPY README.md ./
RUN poetry install --only main --all-extras
RUN poetry build -f wheel
RUN python -m pip install ./dist/*whl --force --no-deps
RUN find /usr/local/lib/python3.12/site-packages -name "*.pyc" -exec rm -f {} \;

# Final application image
FROM base as application

COPY --from=builder /usr/local/lib/python3.12/site-packages /usr/local/lib/python3.12/site-packages
COPY examples ./examples
