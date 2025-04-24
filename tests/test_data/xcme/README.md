# File: `6EH4.XCME_1min_bars_20240101_20240131.csv.gz`

- Instrument: 6E
- Expiration: H4 (March 2024)
- Exchange:   XCME (MIC code)
- Period      2024-01-01 --> 2024-01-31 (UTC timestamp, no contract rollover occurs in this period)
- Bar type:   1-minute bars

# Zipped format

We used zipped data, because they are 9x smaller than original CSV file and can be DIRECTLY read by [pandas](https://pandas.pydata.org/)
using code like this:

```python
import pandas as pd

df = pd.read_csv(
    "6EH4.XCME_1min_bars_20240101_20240131.csv.gz",  # update path as needed
    header=0,
    index_col=False,
)
```
