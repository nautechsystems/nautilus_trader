from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import as_completed
from typing import Callable, List

from tqdm import tqdm

from nautilus_trader.serialization.catalog.parsers import ByteParser
from nautilus_trader.serialization.catalog.scanner import ChunkedFile


# TODO - Add callable for writing chunk filename


def _parse(f: ChunkedFile, parser: ByteParser, instrument_provider=None):
    data = []
    for chunk in parser.read(stream=f.iter_chunks(), instrument_provider=instrument_provider):
        if chunk:
            data.append(chunk)
    return data


def read_files(files: List[ChunkedFile], parser: Callable, progress=True, executor=None):
    executor = executor or ThreadPoolExecutor()

    # Submit files for processing
    futures = []
    with executor as client:
        for f in files:
            futures.append(client.submit(_parse, f=f, parser=parser))

    # Gather results
    if progress:
        futures = tqdm(futures)
    for f in as_completed(futures):
        data = f.result()
        # TODO - write to file

    # stream = self.parser.read(
    #     stream=self.stream_bytes(progress=progress),
    #     instrument_provider=self.instrument_provider,
    # )
    # while 1:
    #     chunk = list(takewhile(lambda x: x is not None, stream))
    #     if chunk and isinstance(chunk[-1], EOStream):
    #         break
    #     if not chunk:
    #         continue
    #     # TODO (bm): shithacks - We need a better way to generate instruments?
    #     if self.instrument_provider is not None:
    #         # Find any instrument status updates, if we have some, emit instruments first
    #         instruments = [
    #             self.instrument_provider.find(s.instrument_id)
    #             for s in chunk
    #             if isinstance(s, InstrumentStatusUpdate)
    #         ]
    #         chunk = instruments + chunk
    #     yield chunk


#
#
#
# def stream_bytes(
#         fs,
#         path,
#         file_filter=None,
#         compression="infer",
#         progress=False,
#         chunk_size=-1,
# ):
#     path = path if not progress else tqdm(path)
#     for fn in filter(file_filter, path):
#         with fs.open(f"{fn}", compression=compression) as f:
#             yield NewFile(fn)
#             data = 1
#             while data:
#                 try:
#                     data = f.read(chunk_size)
#                 except OSError as e:
#                     print(f"ERR file: {fn} ({e}), skipping")
#                     break
#                 yield data
#                 yield None  # this is a chunk
#     yield EOStream()


# def import_from_data_loader(loader, append_only=False, progress=False, **kwargs):
#     """
#     Load data from a DataLoader instance into the backtest catalogue.
#
#     Parameters
#     ----------
#     loader : DataLoader
#         The data loader to use.
#     append_only : bool
#         If read existing data to dedupe + sort.
#         Use this if the data is strictly ordered.
#     kwargs : dict
#         The kwargs passed through to `ParquetWriter`.
#
#     """
#     for chunk in loader.run(progress=progress):
#         self._write_chunks(chunk=chunk, append_only=append_only, **kwargs)
#
#
# def _save_processed_raw_files(files):
#     # TODO(bm): We should save a hash of the contents alongside the filename to check for changes
#     # load existing
#     existing = self._load_processed_raw_files()
#     new = set(files + existing)
#     with self.fs.open(self._processed_files_fn, "wb") as f:
#         return f.write(orjson.dumps(sorted(new)))
#
#
# def _load_processed_raw_files(self):
#     if self.fs.exists(self._processed_files_fn):
#         with self.fs.open(self._processed_files_fn, "rb") as f:
#             return orjson.loads(f.read())
#     else:
#         return []
#
#
# def _determine_partition_cols(cls, instrument_id):
#     if self._partition_keys.get(cls) is not None:
#         return list(self._partition_keys[cls])
#     elif instrument_id is not None:
#         return ["instrument_id"]
#     return
#
#
# def _write_chunks(chunk, append_only=False, **kwargs):  # noqa: C901
#     processed_raw_files = self._load_processed_raw_files()
#     log_filenames = kwargs.pop("log_filenames", False)
#
#     # Split objects into their respective tables
#     type_conv = {OrderBookDeltas: OrderBookDelta, OrderBookSnapshot: OrderBookDelta}
#     tables = defaultdict(dict)
#     skip_file = False
#     for obj in chunk:
#         if skip_file:
#             continue
#         if isinstance(obj, NewFile):
#             if log_filenames:
#                 print(obj.name)
#             if obj.name in processed_raw_files:
#                 skip_file = True
#             else:
#                 skip_file = False
#                 processed_raw_files.append(obj.name)
#             continue
#
#         cls = type_conv.get(type(obj), type(obj))
#         if isinstance(obj, GenericData):
#             cls = obj.data_type.type
#         for data in maybe_list(ParquetSerializer.serialize(obj)):
#             instrument_id = data.get("instrument_id", None)
#             if instrument_id not in tables[cls]:
#                 tables[cls][instrument_id] = []
#             tables[cls][instrument_id].append(data)
#
#     for cls in tables:
#         for ins_id in tables[cls]:
#             name = f"{class_to_filename(cls)}.parquet"
#             fn = self.path.joinpath(name)
#
#             df = pd.DataFrame(tables[cls][ins_id])
#
#             if df.empty:
#                 continue
#
#             ts_col = None
#             for c in NAUTILUS_TS_COLUMNS:
#                 if c in df.columns:
#                     ts_col = c
#             assert ts_col is not None, f"Could not find timestamp column for type: {cls}"
#
#             partition_cols = self._determine_partition_cols(cls=cls, instrument_id=ins_id)
#
#             # Load any existing data, drop dupes
#             if not append_only:
#                 if self.fs.exists(str(fn)) or self.fs.isdir(str(fn)):
#                     existing = self._query(
#                         filename=class_to_filename(cls),
#                         instrument_ids=ins_id if cls not in Instrument.__subclasses__() else None,
#                         ts_column=ts_col,
#                         raise_on_empty=False,
#                     )
#                     if not existing.empty:
#                         df = df.append(existing).drop_duplicates()
#                         # Remove this file/partition, will be written again
#                         if partition_cols:
#                             assert partition_cols == [
#                                 "instrument_id"
#                             ], "Only support appending to instrument_id partitions"
#                             # We only want to remove this partition
#                             partition_path = f"instrument_id={clean_key(ins_id)}"
#                             self.fs.rm(str(fn / partition_path), recursive=True)
#                         else:
#                             self.fs.rm(str(fn), recursive=True)
#
#             df = df.sort_values(ts_col)
#             df = df.astype({k: "category" for k in category_attributes.get(cls.__name__, [])})
#             for col in NAUTILUS_TS_COLUMNS:
#                 if col in df.columns:
#                     df = df.sort_values(col)
#                     break
#
#             df, mappings = clean_partition_cols(df, partition_cols, cls)
#             schema = self._schemas.get(cls)
#             table = pa.Table.from_pandas(df, schema=schema)
#             metadata_collector = []
#             pq.write_to_dataset(
#                 table=table,
#                 root_path=str(fn),
#                 filesystem=self.fs,
#                 partition_cols=partition_cols,
#                 # use_legacy_dataset=True,
#                 version="2.0",
#                 metadata_collector=metadata_collector,
#                 **kwargs,
#             )
#             # Write the ``_common_metadata`` parquet file without row groups statistics
#             pq.write_metadata(
#                 table.schema, str(fn / "_common_metadata"), version="2.0", filesystem=self.fs
#             )
#
#             # Write the ``_metadata`` parquet file with row groups statistics of all files
#             pq.write_metadata(
#                 table.schema, str(fn / "_metadata"), version="2.0", filesystem=self.fs
#             )
#
#             # Write out any partition columns we had to modify due to filesystem requirements
#             if mappings:
#                 self._write_mappings(fn=fn, mappings=mappings)
#
#         # Save any new processed files
#     self._save_processed_raw_files(files=processed_raw_files)
#
#
# def clear_cache(**kwargs):
#     force = kwargs.get("FORCE", False)
#     if not force:
#         print(
#             "Are you sure you want to clear the WHOLE BACKTEST CACHE?, if so, call clear_cache(FORCE=True)"
#         )
#     else:
#         self.fs.rm(self.path, recursive=True)
