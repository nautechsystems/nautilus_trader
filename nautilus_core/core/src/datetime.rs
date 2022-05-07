const MILLISECONDS_IN_SECOND: u64 = 1_000;
const MICROSECONDS_IN_SECOND: u64 = 1_000_000;
const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;
const NANOSECONDS_IN_MILLISECOND: u64 = 1_000_000;
const NANOSECONDS_IN_MICROSECOND: u64 = 1_000;
const NANOSECONDS_IN_DAY: u64 = 86400 * NANOSECONDS_IN_SECOND;

#[inline]
pub fn nanos_to_secs(nanos: f64) -> f64 {
    nanos / NANOSECONDS_IN_SECOND as f64
}

#[inline]
pub fn nanos_to_millis(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MILLISECOND
}

#[inline]
pub fn nanos_to_micros(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MICROSECOND
}
