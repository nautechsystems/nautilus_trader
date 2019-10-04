# Working With Indicators

An indicator is an object which takes data as input and provides one or more
values as outputs. Within the nautilus_trader system an Indicator is recognized
as an Object type which takes one or more numeric (int, float, double) inputs with the following
parameter names;

- 'point'
- 'price'
- 'mid'
- 'open'
- 'high'
- 'low'
- 'close'
- 'volume'

And/or a datetime type with the following parameter name;
- 'timestamp'

An indicator object following these conventions can originate from any library
and can be wrapped by an IndicatorUpdater for backtesting and live trading.
