ARG GIT_TAG
FROM ghcr.io/nautechsystems/nautilus_trader:$GIT_TAG
COPY --from=ghcr.io/nautechsystems/nautilus_data:main /opt/pysetup/catalog /catalog
RUN pip install jupyterlab datafusion
ENV NAUTILUS_PATH="/"
CMD ["python", "-m", "jupyterlab", "--port=8888", "--no-browser", "--ip=0.0.0.0", "--allow-root", "-NotebookApp.token=''", "--NotebookApp.password=''", "examples/notebooks"]
