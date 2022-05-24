use crate::enums::BarAggregation;
use crate::enums::PriceType;
use nautilus_core::string::string_to_pystr;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use pyo3::ffi;

// use crate::enums::AggregationSource;
// use crate::types::price::Price;
// use crate::identifiers::instrument_id::InstrumentId;
// use nautilus_core::time::Timestamp;

// use crate::types::quantity::Quantity;

#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct BarSpecification {
    pub step: u64,
    pub aggregation: BarAggregation,
    pub price_type: PriceType
}

impl Display for BarSpecification {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{}-{}-{}",
            self.step,
            self.aggregation,
            self.price_type
        )
    }
}

impl Hash for BarSpecification {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.step.hash(state);
        self.aggregation.hash(state);
        self.price_type.hash(state);
    }
}

impl PartialOrd for BarSpecification {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_string().partial_cmp(&other.to_string())
    }

    fn lt(&self, other: &Self) -> bool {
        self.to_string().lt(&other.to_string())
    }

    fn le(&self, other: &Self) -> bool {
        self.to_string().le(&other.to_string())
    }

    fn gt(&self, other: &Self) -> bool {
        self.to_string().gt(&other.to_string())
    }

    fn ge(&self, other: &Self) -> bool {
        self.to_string().ge(&other.to_string())
    }
}

#[no_mangle]
pub unsafe extern "C" fn bar_specification_to_pystr(bar_spec: &BarSpecification) -> *mut ffi::PyObject {
    string_to_pystr(bar_spec.to_string().as_str())
}
#[no_mangle]
pub extern "C" fn bar_specification_free(bar_spec: BarSpecification) {
    drop(bar_spec); // Memory freed here
}

#[no_mangle]
pub extern "C" fn bar_specification_hash(bar_spec: &BarSpecification) -> u64 {
    let mut h = DefaultHasher::new();
    bar_spec.hash(&mut h);
    h.finish()
}

#[no_mangle]
pub extern "C" fn bar_specification_new(
    step: u64,
    aggregation: BarAggregation,
    price_type: PriceType
) -> BarSpecification {
    BarSpecification {
        step,
        aggregation,
        price_type,
    }
}
#[cfg(test)]
mod tests {
    use crate::data::bar::BarSpecification;
    use crate::enums::BarAggregation;
    use crate::enums::PriceType;
    // use std::hash::Hash;

    #[test]
    fn test_bar_spec_equality() {
        // Arrange
        let bar_spec1 = BarSpecification{
                            step: 1,
                            aggregation: BarAggregation::Minute,
                            price_type: PriceType::Bid};
        let bar_spec2 = BarSpecification{
                            step: 1,
                            aggregation: BarAggregation::Minute,
                            price_type: PriceType::Bid};
        let bar_spec3 = BarSpecification{
                            step: 1,
                            aggregation: BarAggregation::Minute,
                            price_type: PriceType::Ask};

        // Act, Assert
        assert_eq!(bar_spec1, bar_spec1);
        assert_eq!(bar_spec1, bar_spec2);
        assert_ne!(bar_spec1, bar_spec3);
    }

    #[test]
    fn test_bar_spec_comparison() {
        // # Arrange
        let bar_spec1 = BarSpecification{
                            step: 1,
                            aggregation: BarAggregation::Minute,
                            price_type: PriceType::Bid
                        };
        let bar_spec2 = BarSpecification{
                            step: 1,
                            aggregation: BarAggregation::Minute,
                            price_type: PriceType::Bid
                        };
        let bar_spec3 = BarSpecification{
                            step: 1,
                            aggregation: BarAggregation::Minute,
                            price_type: PriceType::Ask
                        };

        // # Act, Assert
        assert!(bar_spec1 <= bar_spec2);
        assert!(bar_spec3 < bar_spec1);
        assert!(bar_spec1 > bar_spec3);
        assert!(bar_spec1 >= bar_spec3);
    }


    #[test]
    fn test_string_reprs() {
        let bar_spec = BarSpecification{
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid
        };
        assert_eq!(bar_spec.to_string(), "1-MINUTE-BID");
        assert_eq!(format!("{bar_spec}"), "1-MINUTE-BID");
    }
    
}
