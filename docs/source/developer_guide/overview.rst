Overview
========

We recommend the PyCharm Professional edition IDE as it interprets Cython syntax.
Unfortunately the Community edition will not interpret Cython syntax.

> https://www.jetbrains.com/pycharm/

To run code or tests from the source code, first compile the C extensions for the package.

    $ poetry install


Packaging for PyPI
------------------

### CI Pipeline
The CI pipeline will now automatically package passing builds and upload
to PyPI via twine. The below manual packaging instructions are being kept
for historical reasons.

### Manually Packaging
Ensure version has been bumped and not pre-release.

Create the distribution package wheel and sdist tar.gz.

    poetry install && poetry build


Ensure this is the only distribution in the /dist directory

Push package to PyPI using twine;

    poetry publish

Username is \__token__

Use the pypi token


Internal Data
-------------

Various data is contained internally in the `_data` folder. If file names are
changed ensure the `MANIFEST.in` is updated.

### Libor Rates
The libor rates for 1 month USD can be updated by downloading the CSV data
from https://fred.stlouisfed.org/series/USD1MTD156N

Ensure you select `Max` for the time window.

### Short Term Interest Rates
The interbank short term interest rates can be updated by downloading the CSV
data at https://data.oecd.org/interest/short-term-interest-rates.htm

### Economic Events
The economic events can be updated from downloading the CSV data from fxstreet
https://www.fxstreet.com/economic-calendar

Ensure timezone is set to GMT.

A maximum 3 month range can be filtered and so yearly quarters must be
downloaded manually and stitched together into a single CSV.

Use the calendar icon to filter the data in the following way;

- 01/01/xx - 31/03/xx
- 01/04/xx - 30/06/xx
- 01/07/xx - 30/09/xx
- 01/10/xx - 31/12/xx

Download each CSV
