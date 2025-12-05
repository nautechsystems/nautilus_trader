# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from datetime import UTC
from datetime import datetime
from datetime import timedelta
from pathlib import Path
from typing import Any

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.persistence.catalog import ParquetDataCatalog


# Note: when using the functions below, change the variable below to a folder path
# where you store all your databento data
DATA_PATH = PACKAGE_ROOT / "tests" / "test_data" / "databento"

client = None


def init_databento_client(databento_api_key: str | None = None) -> None:
    """
    Initialize the global Databento historical client.

    If `databento_api_key` is None, an environment variable with the same name
    will be used.

    Parameters
    ----------
    databento_api_key : str, optional
        The Databento API key. If None, will use the databento_api_key environment variable.

    """
    import databento as db

    global client
    client = db.Historical(key=databento_api_key)


def databento_cost(
    symbols: list[str],
    start_time: str,
    end_time: str,
    schema: str,
    dataset: str = "GLBX.MDP3",
    **kwargs: Any,
) -> float:
    """
    Calculate the cost of retrieving data from the Databento API for the given
    parameters.

    Parameters
    ----------
    symbols : list[str]
        The symbols to retrieve data for.
    start_time : str
        The start time of the data in ISO 8601 format.
    end_time : str
        The end time of the data in ISO 8601 format.
    schema : str
        The data schema to retrieve.
    dataset : str, optional
        The Databento dataset to use, defaults to "GLBX.MDP3".
    **kwargs : Any
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
    symbols: list[str],
    start_time: str,
    end_time: str,
    schema: str,
    file_prefix: str,
    *folders: str,
    dataset: str = "GLBX.MDP3",
    to_catalog: bool = True,
    base_path: Path | None = None,
    use_exchange_as_venue: bool = True,
    load_databento_files_if_exist: bool = False,
    as_legacy_cython: bool = False,
    **kwargs: Any,
) -> dict[str, Any]:
    """
    Download and save Databento data and definition files, and optionally save the data
    to a catalog.

    Parameters
    ----------
    symbols : list[str]
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
    base_path : Path, optional
        The base path to use for the data folder, defaults to None.
    use_exchange_as_venue : bool, optional
        Whether to use actual exchanges for instrument ids or GLBX, defaults to True.
    load_databento_files_if_exist : bool, optional
        Whether to load Databento files if they already exist, defaults to False.
    as_legacy_cython : bool, optional
        Whether to use legacy Cython format, defaults to False.
    **kwargs : Any
        Additional keyword arguments to pass to the Databento API.

    Returns
    -------
    dict[str, Any]
        A dictionary containing the downloaded data and metadata with keys:
        - symbols: The list of symbols
        - dataset: The dataset name
        - schema: The schema name
        - start: The start time
        - end: The end time
        - databento_definition_file: Path to definition file
        - databento_data_file: Path to data file (if schema != "definition")
        - databento_definition: Definition data object
        - databento_data: Data object (if schema != "definition")
        - catalog: Catalog object (if to_catalog=True)
        - nautilus_definition: Processed definition data (if to_catalog=True)
        - nautilus_data: Processed market data (if to_catalog=True)

    Notes
    -----
    If schema is equal to 'definition' then no data is downloaded or saved to the catalog.

    """
    used_path = create_data_folder(*folders, "databento", base_path=base_path)
    definition_start_date, definition_end_date = databento_definition_dates(start_time)
    definition_file_name = f"{file_prefix}_definition.dbn.zst"
    definition_file = used_path / definition_file_name
    if not definition_file.exists():
        if client is None:
            raise ValueError("Databento client not initialized. Call init_databento_client() first.")

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
        definition = load_databento_data(definition_file) if load_databento_files_if_exist else None

    data = None
    data_file_name = f"{file_prefix}_{schema}_{start_time}_{end_time}.dbn.zst".replace(":", "h")
    data_file = used_path / data_file_name

    if schema != "definition":
        if not data_file.exists():
            if client is None:
                raise ValueError("Databento client not initialized. Call init_databento_client() first.")

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
            data = load_databento_data(data_file) if load_databento_files_if_exist else None

    result = {
        "symbols": symbols,
        "dataset": dataset,
        "schema": schema,
        "start": start_time,
        "end": end_time,
        "databento_definition_file": definition_file,
        "databento_data_file": data_file if schema != "definition" else None,
        "databento_definition": definition,
        "databento_data": data,
    }

    if schema == "definition":
        del result["databento_data_file"]
        del result["databento_data"]

    if to_catalog:
        catalog_data = save_data_to_catalog(
            *folders,
            definition_file=definition_file,
            data_file=data_file,
            base_path=base_path,
            use_exchange_as_venue=use_exchange_as_venue,
            as_legacy_cython=as_legacy_cython,
        )
        result.update(catalog_data)

    return result


def databento_definition_dates(start_time: str) -> tuple[str, str]:
    """
    Calculate definition date and end date from a start time string.

    Extracts the date portion from an ISO 8601 datetime string and calculates
    the next day as the end date.

    Parameters
    ----------
    start_time : str
        The start time in ISO 8601 format (e.g., "2024-01-01T00:00:00Z").

    Returns
    -------
    tuple[str, str]
        A tuple containing (definition_date, end_date) in "YYYY-MM-DD" format.

    """
    definition_date = start_time.split("T")[0]
    used_end_date = next_day(definition_date)

    return definition_date, used_end_date


def next_day(date_str: str) -> str:
    """
    Calculate the next day from a date string.

    Parameters
    ----------
    date_str : str
        The date string in "YYYY-MM-DD" format.

    Returns
    -------
    str
        The next day in "YYYY-MM-DD" format.

    """
    date_format = "%Y-%m-%d"
    date = datetime.strptime(date_str, date_format).replace(tzinfo=UTC)
    result = date + timedelta(days=1)

    return result.strftime(date_format)


def save_data_to_catalog(
    *folders: str,
    definition_file: Path | str | None = None,
    data_file: Path | str | None = None,
    base_path: Path | None = None,
    use_exchange_as_venue: bool = True,
    as_legacy_cython: bool = False,
) -> dict[str, Any]:
    """
    Save Databento data to a catalog.

    This function loads a catalog, processes Databento definition and data files,
    and writes the processed data to the catalog.

    Parameters
    ----------
    *folders : str
        The variable length argument list of folder names to be used in the catalog path.
    definition_file : Path | str, optional
        The path to the Databento definition file.
    data_file : Path | str, optional
        The path to the Databento data file.
    base_path : Path, optional
        The base path for the catalog.
    use_exchange_as_venue : bool, optional
        Whether to use actual exchanges for instrument IDs or GLBX, defaults to True.
    as_legacy_cython : bool, optional
        Whether to use legacy Cython format, defaults to False.

    Returns
    -------
    dict[str, Any]
        A dictionary containing:
        - 'catalog': The loaded catalog object.
        - 'nautilus_definition': Processed Databento definition data (or None if definition_file not provided).
        - 'nautilus_data': Processed Databento market data (or None if data_file not provided).

    Notes
    -----
    - If definition_file is provided, it will be processed and written to the catalog.
    - If data_file is provided, it will be processed and written to the catalog.
    - The function uses DatabentoDataLoader to process the files.

    """
    catalog = load_catalog(*folders, base_path=base_path)

    loader = DatabentoDataLoader()

    if definition_file is not None:
        nautilus_definition = loader.from_dbn_file(
            definition_file,
            as_legacy_cython=True,
            use_exchange_as_venue=use_exchange_as_venue,
        )
        catalog.write_data(nautilus_definition)
    else:
        nautilus_definition = None

    if data_file is not None:
        nautilus_data = loader.from_dbn_file(
            data_file,
            as_legacy_cython=as_legacy_cython,
        )
        catalog.write_data(nautilus_data)
    else:
        nautilus_data = None

    return {
        "catalog": catalog,
        "nautilus_definition": nautilus_definition,
        "nautilus_data": nautilus_data,
    }


def load_catalog(*folders: str, base_path: Path | None = None) -> ParquetDataCatalog:
    """
    Load a ParquetDataCatalog from the specified folders and base path.

    Parameters
    ----------
    *folders : str
        The folders to create the data path from.
    base_path : Path, optional
        The base path to use for the data folder, defaults to None.

    Returns
    -------
    ParquetDataCatalog
        The loaded ParquetDataCatalog.

    """
    catalog_path = create_data_folder(*folders, base_path=base_path)

    return ParquetDataCatalog(catalog_path)


def create_data_folder(*folders: str, base_path: Path | None = None) -> Path:
    """
    Create a data folder at the specified path.

    Creates the directory structure if it doesn't exist.

    Parameters
    ----------
    *folders : str
        The folders to include in the path.
    base_path : Path, optional
        The base path to use, defaults to `DATA_PATH`.

    Returns
    -------
    Path
        The path to the created data folder.

    """
    used_path = data_path(*folders, base_path=base_path)

    if not used_path.exists():
        used_path.mkdir(parents=True, exist_ok=True)

    return used_path


def data_path(*folders: str, base_path: Path | None = None) -> Path:
    """
    Get the path to a data folder.

    Parameters
    ----------
    *folders : str
        The folders to include in the path.
    base_path : Path, optional
        The base path to use, defaults to `DATA_PATH`.

    Returns
    -------
    Path
        The full path to the data folder.

    """
    used_base_path = base_path if base_path is not None else DATA_PATH
    result = used_base_path

    for folder in folders:
        result /= folder

    return result


def query_catalog(
    catalog: ParquetDataCatalog,
    data_type: str = "bars",
    **kwargs: Any,
) -> Any:
    """
    Query a catalog for different types of data.

    Parameters
    ----------
    catalog : ParquetDataCatalog
        The catalog to query.
    data_type : str, optional
        The type of data to query. Valid values are "bars", "ticks", "instruments", or "custom", defaults to "bars".
    **kwargs : Any
        Additional keyword arguments to pass to the catalog query method.

    Returns
    -------
    Any
        The query results, type depends on data_type:
        - "bars": Returns bar data
        - "ticks": Returns quote tick data
        - "instruments": Returns instrument data
        - "custom": Returns custom data

    """
    if data_type == "bars":
        return catalog.bars(**kwargs)
    elif data_type == "ticks":
        return catalog.quote_ticks(**kwargs)
    elif data_type == "instruments":
        return catalog.instruments(**kwargs)
    elif data_type == "custom":
        return catalog.custom_data(**kwargs)
    else:
        raise ValueError(f"Invalid data_type: {data_type}. Must be one of 'bars', 'ticks', 'instruments', 'custom'")


def load_databento_data(file: Path | str) -> Any:
    """
    Load Databento data from a DBN file.

    Parameters
    ----------
    file : Path | str
        The path to the DBN file to load.

    Returns
    -------
    Any
        A DBNStore object containing the loaded data.

    """
    import databento as db

    return db.DBNStore.from_file(file)


def save_databento_data(data: Any, file: Path | str) -> Any:
    """
    Save Databento data to a file.

    Parameters
    ----------
    data : Any
        The Databento data object to save (typically a DBNStore).
    file : Path | str
        The path to save the data file to.

    Returns
    -------
    Any
        The result of the save operation (typically the file path).

    """
    return data.to_file(file)
