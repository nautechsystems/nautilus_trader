from numpy import ndarray

def fast_mean(values: ndarray) -> float:
    """
    Return the average value for numpy.ndarray values.

    Parameters
    ----------
    values : numpy.ndarray
        The array to evaluate.

    Returns
    -------
    double

    Notes
    -----
    > 10x faster than `np.mean` if the array length < ~200.

    """
    ...
def fast_mean_iterated(values: ndarray, next_value: float, current_value: float, expected_length: int, drop_left: bool = True) -> float:
    """
    Return the calculated average from the given inputs.

    Parameters
    ----------
    values : list[double]
        The values for the calculation.
    next_value : double
        The next input value for the average.
    current_value : double
        The current value for the average.
    expected_length : int
        The expected length of the inputs.
    drop_left : bool
        If the value to be dropped should be from the left side of the inputs
        (index 0).

    Returns
    -------
    double

    Notes
    -----
    > 10x faster than `np.mean`.

    """
    ...
def fast_std(values: ndarray) -> float:
    """
    Return the standard deviation from the given values.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.

    Returns
    -------
    double

    Notes
    -----
    > 10x faster than `np.std`.

    """
    ...
def fast_std_with_mean(values: ndarray, mean: float) -> float:
    """
    Return the standard deviation from the given values and mean.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.
    mean : double
        The pre-calculated mean of the given values.

    Returns
    -------
    double

    Notes
    -----
    > 25x faster than `np.std` if the array length < ~200.

    """
    ...
def fast_mad(values: ndarray) -> float:
    """
    Return the mean absolute deviation from the given values.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.

    Returns
    -------
    double

    """
    ...
def fast_mad_with_mean(values: ndarray, mean: float) -> float:
    """
    Return the mean absolute deviation from the given values and mean.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.
    mean : double
        The pre-calculated mean of the given values.

    Returns
    -------
    double

    """
    ...
def basis_points_as_percentage(basis_points: float) -> float:
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.

    Parameters
    ----------
    basis_points : double
        The basis points to convert to percentage.

    Returns
    -------
    double

    Notes
    -----
    1 basis point = 0.01%.

    """
    ...
