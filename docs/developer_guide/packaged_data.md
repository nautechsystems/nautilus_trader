# Packaged Data

Various data is contained internally in the `tests/test_kit/data` folder.

## Libor rates

The libor rates for 1 month USD can be updated by downloading the CSV data from <https://fred.stlouisfed.org/series/USD1MTD156N>.

Ensure you select `Max` for the time window.

## Short term interest rates

The interbank short term interest rates can be updated by downloading the CSV data at <https://data.oecd.org/interest/short-term-interest-rates.htm>.

## Economic events

The economic events can be updated from downloading the CSV data from fxstreet <https://www.fxstreet.com/economic-calendar>.

Ensure timezone is set to GMT.

A maximum 3 month range can be filtered and so yearly quarters must be downloaded manually and stitched together into a single CSV.
Use the calendar icon to filter the data in the following way;

- 01/01/xx - 31/03/xx
- 01/04/xx - 30/06/xx
- 01/07/xx - 30/09/xx
- 01/10/xx - 31/12/xx

Download each CSV.
