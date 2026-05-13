ARG GIT_TAG=develop
FROM ghcr.io/nautechsystems/nautilus_trader:$GIT_TAG

COPY docs/tutorials /opt/pysetup/tutorials

ENV PATH="/root/.local/bin:$PATH"

# Install build deps
RUN apt-get update && \
    apt-get install -y curl && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL --retry 3 \
      -o /tmp/eurusd_quotes.parquet \
      "https://test-data.nautechsystems.io/large/histdata_EURUSD.SIM_2020-01_quotes.parquet" && \
    printf '%s  %s\n' \
      "9c610a233b8408562ea9024df0bd3192608f16ed00fce6f5d761a321a3d897c2" \
      "/tmp/eurusd_quotes.parquet" | sha256sum -c - && \
    curl -fsSL --retry 3 \
      -o /tmp/eurusd_instrument.parquet \
      "https://test-data.nautechsystems.io/large/histdata_EURUSD.SIM_2020-01_instrument.parquet" && \
    printf '%s  %s\n' \
      "2088959dc15eecfebb7d4c45054d6a74d1000078daa1153388fe19c3b1468bac" \
      "/tmp/eurusd_instrument.parquet" | sha256sum -c - && \
    mkdir -p /catalog/data/quote_tick/EURUSD.SIM /catalog/data/currency_pair/EURUSD.SIM && \
    mv /tmp/eurusd_quotes.parquet /catalog/data/quote_tick/EURUSD.SIM/part-0.parquet && \
    mv /tmp/eurusd_instrument.parquet /catalog/data/currency_pair/EURUSD.SIM/part-0.parquet

# Install UV
COPY scripts/uv-version.sh scripts/
COPY pyproject.toml ./
RUN UV_VERSION=$(bash scripts/uv-version.sh) && curl -LsSf https://astral.sh/uv/$UV_VERSION/install.sh | sh

RUN uv pip install --system jupyterlab datafusion

ENV NAUTILUS_PATH="/"

CMD ["python", "-m", "jupyterlab", "--port=8888", "--no-browser", "--ip=0.0.0.0", "--allow-root", "-NotebookApp.token=''", "--NotebookApp.password=''", "tutorials"]
