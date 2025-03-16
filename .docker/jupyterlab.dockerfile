ARG GIT_TAG=develop
FROM ghcr.io/nautechsystems/nautilus_trader:$GIT_TAG

COPY --from=ghcr.io/nautechsystems/nautilus_data:main /opt/pysetup/catalog /catalog
COPY docs/tutorials /opt/pysetup/tutorials

ENV PATH="/root/.local/bin:$PATH"

# Install build deps
RUN apt-get update && \
    apt-get install -y curl && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install UV
COPY uv-version ./
RUN UV_VERSION=$(cat uv-version) && curl -LsSf https://astral.sh/uv/$UV_VERSION/install.sh | sh

RUN uv pip install --system jupyterlab datafusion

ENV NAUTILUS_PATH="/"

CMD ["python", "-m", "jupyterlab", "--port=8888", "--no-browser", "--ip=0.0.0.0", "--allow-root", "-NotebookApp.token=''", "--NotebookApp.password=''", "tutorials"]
