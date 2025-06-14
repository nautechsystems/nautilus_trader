class ParquetDataCatalog:
    def __init__(self, path, fs_protocol, fs_storage_options):
        self.path = path
        self.fs_protocol = fs_protocol
        self.fs_storage_options = fs_storage_options

    def backend_session(self, data_cls):
        if self.fs_protocol == "s3":
            raise AssertionError(
                "Remote catalogs (e.g. S3) are not supported for Rust queries. "
                "Please use a local catalog (e.g. a local parquet file) for Rust queries. "
                "For remote catalogs, use a Python query (e.g. query_py) instead."
            )

# Simulate a remote catalog (using a fake S3 endpoint) so that the assertion (or error) is raised.
catalog = ParquetDataCatalog(
    path="fake-bucket/fake-path",  # fake S3 bucket/path
    fs_protocol="s3",
    fs_storage_options={
        "key": "FAKE_KEY",
        "secret": "FAKE_SECRET",
        "client_kwargs": {"endpoint_url": "https://fake-s3-endpoint.amazonaws.com"},
    }
)

# Use a dummy data class
class Bar:
    pass

try:
    catalog.backend_session(data_cls=Bar)
except AssertionError as e:
    print("Caught error:", e) 