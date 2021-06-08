import os
from typing import Generator

import fsspec
import pandas as pd


class FileName:
    def __init__(self, filename):
        self.name = filename


class ByteParser:
    def __init__(
        self,
        instrument_provider_update: callable = None,
    ):
        self.instrument_provider_update = instrument_provider_update

    def read(self, stream: Generator) -> Generator:
        raise NotImplementedError


class CSVParser(ByteParser):
    def read(self, stream: Generator) -> Generator:
        for chunk in stream:
            yield pd.read_csv(chunk)


class TextParser(ByteParser):
    def __init__(
        self,
        line_parser: callable,
        line_preprocessor=None,
        instrument_provider_update=None,
    ):
        """
        Parse bytes of json into nautilus objects

        :param line_parser: Callable that takes a JSON object and yields nautilus objects
        :param line_preprocessor: A context manager for doing any preprocessing (cleaning log lines) of lines before
               json.loads is called. Nautilus objects are returned to the context manager for any post-processing also
               (For example, setting the `timestamp_origin_ns`)
        :param instrument_provider_update (Optional) : An optional hook/callable to update instrument provider before
               data is passed to `line_parser` (in many cases instruments need to be known ahead of parsing)
        """
        self.line_parser = line_parser
        self.line_preprocessor = line_preprocessor
        super().__init__(instrument_provider_update=instrument_provider_update)

    def read(self, stream: Generator) -> Generator:
        raw = b""
        fn = None
        for chunk in stream:
            if isinstance(chunk, FileName):
                # New file, yield any remaining data, reset bytes
                assert not raw, f"Data remaining at end of file: {fn}"
                fn = chunk.name
                raw = b""
                continue
            if chunk == b"":
                # This is probably EOF? Append a newline to ensure we emit the previous line
                chunk = b"\n"
            raw += chunk

            lines = raw.split(b"\n")
            raw = lines[-1]
            for line in lines[:-1]:
                if not line:
                    continue
                if self.instrument_provider_update is not None:
                    self.instrument_provider_update(line)
                yield from self.line_parser(line)


class ParquetParser(ByteParser):
    def read(self, stream: Generator) -> Generator:
        for chunk in stream:
            df = pd.read_parquet(chunk)
            yield df


class DataLoader:
    def __init__(
        self,
        path: str,
        parser: ByteParser,
        fs_proto="file",
        glob_pattern="**",
        progress=False,
        line_preprocessor: callable = None,
        chunksize=-1,
        compression="infer",
        instrument_loader: callable = None,
    ):
        """
        Discover files and stream bytes of data from a local or remote filesystem

        :param path: A resolvable path; a file, folder, or a remote location via fsspec
        :param parser: A `BaseReader` subclass that can convert bytes into nautilus objects.
        :param fs_proto: fsspec protocol; allows remote access - defaults to `file`
        :param progress: Show progress when loading individual files
        :param glob_pattern: Glob pattern to search for files
        :param glob_pattern: Glob pattern to search for files
        :param compression: The file compression, defaults to 'infer' by file extension
        :param line_preprocessor: A callable that handles any preprocessing of the data
        :param chunksize: Chunk size (in bytes) for processing data, -1 for no limit (will chunk per file).
        """
        self._path = path
        self.parser = parser
        self.fs_proto = fs_proto
        self.progress = progress
        self.chunk_size = chunksize
        self.compression = compression
        self.glob_pattern = glob_pattern
        self.line_preprocessor = line_preprocessor
        self.fs = fsspec.filesystem(self.fs_proto)
        self.instrument_loader = instrument_loader
        self.path = sorted(self._resolve_path(path=self._path))

    def _resolve_path(self, path: str):
        if self.fs.isfile(path):
            return [path]
        # We have a directory
        if not path.endswith("/"):
            path += "/"
        if self.fs.isdir(path):
            files = self.fs.glob(f"{path}{self.glob_pattern}")
            assert (
                files
            ), f"Found no files with path={str(path)}, glob={self.glob_pattern}"
            return [f for f in files if self.fs.isfile(f)]
        else:
            raise ValueError("path argument must be str and a valid directory or file")

    def stream_bytes(self):
        for fn in self.path:
            with self.fs.open(fn, compression=self.compression) as f:
                yield FileName(fn)
                data = 1
                while data:
                    data = f.read(self.chunk_size)
                    yield data

    def run(self):
        for chunk in self.parser.read(self.stream_bytes()):
            # TODO - is this reqiured?
            if isinstance(chunk, FileName):
                pass
            else:
                yield chunk


# def write_chunks():
#     import pandas as pd
#     import pyarrow as pa
#     import pyarrow.parquet as pq
#
#     chunksize = 10000  # this is the number of lines
#
#     pqwriter = None
#     for i, df in enumerate(pd.read_csv("sample.csv", chunksize=chunksize)):
#         table = pa.Table.from_pandas(df)
#         # for the first chunk of records
#         if i == 0:
#             # create a parquet write object giving it an output file
#             pqwriter = pq.ParquetWriter("sample.parquet", table.schema)
#         pqwriter.write_table(table)
#
#     # close the parquet writer
#     if pqwriter:
#         pqwriter.close()


class DataCatalogue:
    def __init__(self):
        self.root = os.environ["NAUTILUS_BACKTEST_DIR"]

    def open_parquet(self, fn):
        return
