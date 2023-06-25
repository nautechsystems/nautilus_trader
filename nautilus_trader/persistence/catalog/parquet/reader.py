import fsspec


class ParquetReader:
    def __init__(
        self,
        fs: fsspec.filesystem = fsspec.filesystem("file"),
    ):
        self._fs = fs

    def query(self, **kwargs) -> None:
        pass
