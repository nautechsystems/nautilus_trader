# Working With Indicators

An indicator is an object which takes data as input and provides one or more
values as outputs. Within the nautilus_trader system an Indicator is recognized
as an Object type which takes one or more numeric inputs with the following
parameter names;

- 'point'
- 'price'
- 'mid'
- 'open'
- 'high'
- 'low'
- 'close'
- 'volume'

Or a datetime type with the following parameter name;
- 'timestamp'

This indicator object following these conventions can originate from any library
and be wrapped by an IndicatorUpdater.
