from datetime import datetime
from datetime import timedelta
from pathlib import Path

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.persistence.catalog import ParquetDataCatalog


DATA_PATH = Path("~/databento_data").expanduser()

# this variable can be modified with a valid key if downloading data is needed
DATABENTO_API_KEY = "db-XXXXX"


client = None


def init_databento_client():
    import databento as db

    global client
    client = db.Historical(key=DATABENTO_API_KEY)


def data_path(*folders, base_path=None):
    used_base_path = base_path if base_path is not None else DATA_PATH
    result = used_base_path

    for folder in folders:
        result /= folder

    return result


def create_data_folder(*folders, base_path=None):
    used_path = data_path(*folders, base_path=base_path)

    if not used_path.exists():
        used_path.mkdir(parents=True, exist_ok=True)

    return used_path


def databento_definition_dates(start_time):
    definition_date = start_time.split("T")[0]
    used_end_date = next_day(definition_date)

    return definition_date, used_end_date


def databento_cost(symbols, start_time, end_time, schema, dataset="GLBX.MDP3", **kwargs):
    definition_start_date, definition_end_date = databento_definition_dates(start_time)

    return client.metadata.get_cost(
        dataset=dataset,
        symbols=symbols,
        schema=schema,
        start=(definition_start_date if schema == "definition" else start_time),
        end=(definition_end_date if schema == "definition" else end_time),
        **kwargs,
    )


def databento_data(
    symbols,
    start_time,
    end_time,
    schema,
    file_prefix,
    *folders,
    dataset="GLBX.MDP3",
    to_catalog=True,
    base_path=None,
    **kwargs,
):
    """
    If schema is equal to definition then no data is downloaded or saved to the catalog.
    """
    used_path = create_data_folder(*folders, "databento", base_path=base_path)

    # downloading and saving definition
    definition_start_date, definition_end_date = databento_definition_dates(start_time)
    definition_file_name = f"{file_prefix}_definition.dbn.zst"
    definition_file = used_path / definition_file_name

    if not definition_file.exists():
        definition = client.timeseries.get_range(
            schema="definition",
            dataset=dataset,
            symbols=symbols,
            start=definition_start_date,
            end=definition_end_date,
            path=definition_file,
            **kwargs,
        )
    else:
        definition = load_databento_data(definition_file)

    # downloading and saving data
    data_file_name = f"{file_prefix}_{schema}_{start_time}_{end_time}.dbn.zst".replace(":", "h")
    data_file = used_path / data_file_name

    if schema != "definition":
        if not data_file.exists():
            data = client.timeseries.get_range(
                schema=schema,
                dataset=dataset,
                symbols=symbols,
                start=start_time,
                end=end_time,
                path=data_file,
                **kwargs,
            )
        else:
            data = load_databento_data(data_file)
    else:
        data = None

    result = {
        "symbols": symbols,
        "dataset": dataset,
        "schema": schema,
        "start": start_time,
        "end": end_time,
        "databento_definition_file": definition_file,
        "databento_data_file": data_file,
        "databento_definition": definition,
        "databento_data": data,
    }

    if schema == "definition":
        del result["data"]
        del result["data_file"]

    if to_catalog and schema != "definition":
        catalog_data = save_data_to_catalog(
            definition_file,
            data_file,
            *folders,
            base_path=base_path,
        )
        result.update(catalog_data)

    return result


def save_data_to_catalog(definition_file, data_file, *folders, base_path=None):
    catalog = load_catalog(*folders, base_path=base_path)

    loader = DatabentoDataLoader()
    nautilus_definition = loader.from_dbn_file(definition_file, as_legacy_cython=True)
    nautilus_data = loader.from_dbn_file(data_file, as_legacy_cython=False)

    catalog.write_data(nautilus_definition)
    catalog.write_data(nautilus_data)

    return {
        "catalog": catalog,
        "nautilus_definition": nautilus_definition,
        "nautilus_data": nautilus_data,
    }


def load_catalog(*folders, base_path=None):
    catalog_path = create_data_folder(*folders, base_path=base_path)

    return ParquetDataCatalog(catalog_path)


def query_catalog(catalog, data_type="bars", **kwargs):
    if data_type == "bars":
        return catalog.bars(**kwargs)
    elif data_type == "ticks":
        return catalog.quote_ticks(**kwargs)
    elif data_type == "instruments":
        return catalog.instruments(**kwargs)
    elif data_type == "custom":
        return catalog.custom_data(**kwargs)


def load_databento_data(file):
    import databento as db

    return db.DBNStore.from_file(file)


def save_databento_data(data, file):
    return data.to_file(file)


def next_day(date_str):
    date_format = "%Y-%m-%d"
    date = datetime.strptime(date_str, date_format)
    next_day = date + timedelta(days=1)

    return next_day.strftime(date_format)
