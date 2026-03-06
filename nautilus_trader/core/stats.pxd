cimport numpy as np


cpdef double fast_mean(np.ndarray values)
cpdef double fast_mean_iterated(
    np.ndarray values,
    double next_value,
    double current_value,
    int expected_length,
    bint drop_left=*,
)
cpdef double fast_std(np.ndarray values)
cpdef double fast_std_with_mean(np.ndarray values, double mean)
cpdef double fast_mad(np.ndarray values)
cpdef double fast_mad_with_mean(np.ndarray values, double mean)
cpdef double basis_points_as_percentage(double basis_points)
