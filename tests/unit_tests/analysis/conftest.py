import pandas as pd


def convert_series_to_dict(series: pd.Series) -> dict[int, float]:
    """
    Convert pandas Series to dict with unix nanoseconds (or integer keys).
    """
    if series.empty:
        return {}
    result = {}
    for idx, val in series.items():
        # Check if index is datetime (has .value attribute for nanoseconds)
        if hasattr(idx, "value"):
            key = idx.value  # Direct nanosecond value, no float precision loss
        else:
            # Use integer index directly (convert to nanoseconds for consistency)
            key = int(idx) * 1_000_000_000
        result[key] = float(val)
    return result
