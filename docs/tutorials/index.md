# Tutorials

The tutorials provide a guided learning experience with a series of comprehensive step-by-step walkthroughs.
Each tutorial targets specific features or workflows, enabling hands-on learning.
From basic tasks to more advanced operations, these tutorials cater to a wide range of skill levels.

:::info
Each tutorial is generated from a Jupyter notebook located in the docs [tutorials directory](https://github.com/nautechsystems/nautilus_trader/tree/develop/docs/tutorials).
These notebooks not only serve as valuable learning aids but also lets you to execute the code.
:::

:::tip
Make sure you are following the tutorial docs which match the version of NautilusTrader you are running:
- **Latest**: These docs are built from the HEAD of the `master` branch and work with the latest stable release.
- **Nightly**: These docs are built from the HEAD of the `nightly` branch and work with bleeding edge and experimental changes/features currently in development.
:::

## Running in docker
Alternatively, a self-contained dockerized Jupyter notebook server is available for download, which does not require any setup or
installation. This is the fastest way to get up and running to try out NautilusTrader. Bear in mind that any data will be 
deleted when the container is deleted.

- To get started, install docker:
  - Go to [docker.com](https://docs.docker.com/get-docker/) and follow the instructions 
- From a terminal, download the latest image
  - `docker pull ghcr.io/nautechsystems/jupyterlab:nightly --platform linux/amd64`
- Run the docker container, exposing the jupyter port: 
  - `docker run -p 8888:8888 ghcr.io/nautechsystems/jupyterlab:nightly`
- Open your web browser to `localhost:{port}`
  - https://localhost:8888
 
:::info
NautilusTrader currently exceeds the rate limit for Jupyter notebook logging (stdout output),
this is why `log_level` in the examples is set to `ERROR`. If you lower this level to see
more logging then the notebook will hang during cell execution. A fix is currently
being investigated which involves either raising the configured rate limits for
Jupyter, or throttling the log flushing from Nautilus.

- https://github.com/jupyterlab/jupyterlab/issues/12845
- https://github.com/deshaw/jupyterlab-limit-output
:::
