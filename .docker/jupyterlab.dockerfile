ARG BASE_IMAGE_REPOSITORY=ghcr.io/nautechsystems/nautilus_trader:latest
ARG BASE_IMAGE_DIGEST=ffaf4104402164d483371ecb27e21b16231293c416adb9771f9bd97a04f27673
FROM ${BASE_IMAGE_REPOSITORY}@sha256:${BASE_IMAGE_DIGEST}

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
COPY --from=ghcr.io/astral-sh/uv:0.11.21@sha256:ff07b86af50d4d9391d9daf4ff89ce427bc544f9aae87057e69a1cc0aa369946 \
  /uv /uvx /root/.local/bin/

RUN uv pip install --system jupyterlab datafusion

ENV NAUTILUS_PATH="/"

CMD ["python", "-m", "jupyterlab", "--port=8888", "--no-browser", "--ip=0.0.0.0", "--allow-root", "-NotebookApp.token=''", "--NotebookApp.password=''", "tutorials"]
