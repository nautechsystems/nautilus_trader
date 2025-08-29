# Tutorials

The tutorials provide a guided learning experience with a series of comprehensive step-by-step walkthroughs.
Each tutorial targets specific features or workflows, enabling hands-on learning.
From basic tasks to more advanced operations, these tutorials cater to a wide range of skill levels.

:::info
Each tutorial is generated from a Jupyter notebook located in the docs [tutorials directory](https://github.com/nautechsystems/nautilus_trader/tree/develop/docs/tutorials). These notebooks serve as valuable learning aids and let you execute the code interactively.
:::

:::tip

- Make sure you are using the tutorial docs that match your NautilusTrader version:
- **Latest**: These docs are built from the HEAD of the `master` branch and work with the latest stable release. See <https://nautilustrader.io/docs/latest/tutorials/>.
- **Nightly**: These docs are built from the HEAD of the `nightly` branch and work with bleeding-edge and experimental features. See <https://nautilustrader.io/docs/nightly/tutorials/>.

:::

## Running in docker

Alternatively, a self-contained dockerized Jupyter notebook server is available for download, which requires no setup or
installation. This is the fastest way to get up and running to try out NautilusTrader. Note that deleting the container will also delete any data.

- To get started, install docker:
  - Go to [Docker installation guide](https://docs.docker.com/get-docker/) and follow the instructions.
- From a terminal, download the latest image:
  - `docker pull ghcr.io/nautechsystems/jupyterlab:nightly --platform linux/amd64`
- Run the docker container, exposing the Jupyter port:
  - `docker run -p 8888:8888 ghcr.io/nautechsystems/jupyterlab:nightly`
- When the container starts, a URL with an access token will be printed in the terminal. Copy that URL and open it in your browser, for example:
  - <http://localhost:8888>

:::info
NautilusTrader currently exceeds the rate limit for Jupyter notebook logging (stdout output),
therefore we set `log_level` to `ERROR` in the examples. Lowering this level to see
more logging will cause the notebook to hang during cell execution. We are currently
investigating a fix that involves either raising the configured rate limits for
Jupyter, or throttling the log flushing from Nautilus.

- <https://github.com/jupyterlab/jupyterlab/issues/12845>
- <https://github.com/deshaw/jupyterlab-limit-output>

:::
