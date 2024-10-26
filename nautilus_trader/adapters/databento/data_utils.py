from datetime import datetime
from datetime import timedelta

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.persistence.catalog import ParquetDataCatalog


# Note: when using the functions below, change the variable below to a folder path
# where you store all your databento data
DATA_PATH = PACKAGE_ROOT / "tests" / "test_data" / "databento"

# this variable can be modified with a valid key if downloading data is needed
DATABENTO_API_KEY = "db-XXXXX"


client = None


def init_databento_client():
    import databento as db

    global client
    client = db.Historical(key=DATABENTO_API_KEY)


def data_path(*folders, base_path=None):
    """
    Get the path to a data folder, creating it if it doesn't exist.

    Args:
        *folders (str): The folders to include in the path.
        base_path (pathlib.Path, optional): The base path to use, defaults to `DATA_PATH`.

    Returns:
        pathlib.Path: The full path to the data folder.

    """
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


def databento_cost(symbols, start_time, end_time, schema, dataset="GLBX.MDP3", **kwargs) -> float:
    """
    Calculate the cost of retrieving data from the Databento API for the given
    parameters.

    Parameters
    ----------
    symbols : list of str
        The symbols to retrieve data for.
    start_time : str
        The start time of the data in ISO 8601 format.
    end_time : str
        The end time of the data in ISO 8601 format.
    schema : str
        The data schema to retrieve.
    dataset : str, optional
        The Databento dataset to use, defaults to "GLBX.MDP3".
    **kwargs
        Additional keyword arguments to pass to the Databento API.

    Returns
    -------
    float
        The estimated cost of retrieving the data.

    """
    definition_start_date, definition_end_date = databento_definition_dates(start_time)

    return client.metadata.get_cost(  # type: ignore[union-attr]
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
    write_data_mode="overwrite",
    **kwargs,
):
    """
    Download and save Databento data and definition files, and optionally save the data
    to a catalog.

    Parameters
    ----------
    symbols : list of str
        The symbols to retrieve data for.
    start_time : str
        The start time of the data in ISO 8601 format.
    end_time : str
        The end time of the data in ISO 8601 format.
    schema : str
        The data schema to retrieve, either "definition" or another valid schema.
    file_prefix : str
        The prefix to use for the downloaded data files.
    *folders : str
        Additional folders to create in the data path.
    dataset : str, optional
        The Databento dataset to use, defaults to "GLBX.MDP3".
    to_catalog : bool, optional
        Whether to save the data to a catalog, defaults to True.
    base_path : str, optional
        The base path to use for the data folder, defaults to None.
    write_data_mode : str, optional
        Whether to "append", "prepend" or "overwrite" data to an existing catalog, defaults to "overwrite".
    **kwargs
        Additional keyword arguments to pass to the Databento API.

    Returns
    -------
    dict
        A dictionary containing the downloaded data and metadata.

    Notes
    -----
    If schema is equal to 'definition' then no data is downloaded or saved to the catalog.

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
            *folders,
            definition_file=definition_file,
            data_file=data_file,
            base_path=base_path,
            write_data_mode=write_data_mode,
        )
        result.update(catalog_data)

    return result


def save_data_to_catalog(
    *folders,
    definition_file=None,
    data_file=None,
    base_path=None,
    write_data_mode="overwrite",
):
    """
    Save Databento data to a catalog.

    This function loads a catalog, processes Databento definition and data files,
    and writes the processed data to the catalog.

    Parameters
    ----------
    *folders : str
        Variable length argument list of folder names to be used in the catalog path.
    definition_file : str or Path, optional
        Path to the Databento definition file.
    data_file : str or Path, optional
        Path to the Databento data file.
    base_path : str or Path, optional
        Base path for the catalog.
    write_data_mode : str, optional
        Mode for writing data to the catalog. Default is "overwrite".

    Returns
    -------
    dict
        A dictionary containing:
        - 'catalog': The loaded catalog object.
        - 'nautilus_definition': Processed Databento definition data.
        - 'nautilus_data': Processed Databento market data.

    Notes
    -----
    - If definition_file is provided, it will be processed and written to the catalog.
    - If data_file is provided, it will be processed and written to the catalog.
    - The function uses DatabentoDataLoader to process the files.

    """
    catalog = load_catalog(*folders, base_path=base_path)

    loader = DatabentoDataLoader()

    if definition_file is not None:
        nautilus_definition = loader.from_dbn_file(definition_file, as_legacy_cython=True)
        catalog.write_data(nautilus_definition)
    else:
        nautilus_definition = None

    if data_file is not None:
        nautilus_data = loader.from_dbn_file(data_file, as_legacy_cython=False)
        catalog.write_data(nautilus_data, mode=write_data_mode)
    else:
        nautilus_data = None

    return {
        "catalog": catalog,
        "nautilus_definition": nautilus_definition,
        "nautilus_data": nautilus_data,
    }


def load_catalog(*folders, base_path=None):
    """
    Load a ParquetDataCatalog from the specified folders and base path.

    Args:
        *folders (str): The folders to create the data path from.
        base_path (str, optional): The base path to use for the data folder, defaults to None.

    Returns:
        ParquetDataCatalog: The loaded ParquetDataCatalog.

    """
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
    result = date + timedelta(days=1)

    return result.strftime(date_format)
