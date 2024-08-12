use std::any::Any;
use chrono::NaiveDateTime;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::ops::AddAssign;

pub struct PortfolioStatistic;

impl PortfolioStatistic {
    /// The base struct for all portfolio performance statistics.
    /// 
    /// Notes
    /// -----
    /// The return value should be a JSON serializable primitive.

    pub fn fully_qualified_name() -> String {
        /// Return the fully qualified name for the `PortfolioStatistic` struct.
        /// 
        /// Returns
        /// -------
        /// String
        /// 
        /// References
        /// ----------
        /// https://www.python.org/dev/peps/pep-3155/
        
        format!("{}:{}", module_path!(), stringify!(PortfolioStatistic))
    }

    pub fn name(&self) -> String {
        /// Return the name for the statistic.
        /// 
        /// Returns
        /// -------
        /// String
        
        let class_name = std::any::type_name::<Self>();
        let re = Regex::new(r".+?(?:(?<=[a-z])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])|$)").unwrap();
        re.find_iter(class_name)
            .map(|m| m.as_str())
            .collect::<Vec<&str>>()
            .join(" ")
    }

    pub fn calculate_from_returns(&self, returns: &Vec<(NaiveDateTime, f64)>) -> Option<Value> {
        /// Calculate the statistic value from the given raw returns.
        /// 
        /// Parameters
        /// ----------
        /// returns : Vec<(NaiveDateTime, f64)>
        ///     The returns to use for the calculation.
        /// 
        /// Returns
        /// -------
        /// Option<Value>
        ///     A JSON serializable primitive.
        
        // Override in implementation
        None
    }

    pub fn calculate_from_realized_pnls(&self, realized_pnls: &Vec<(NaiveDateTime, f64)>) -> Option<Value> {
        /// Calculate the statistic value from the given raw realized PnLs.
        /// 
        /// Parameters
        /// ----------
        /// realized_pnls : Vec<(NaiveDateTime, f64)>
        ///     The raw PnLs for the calculation.
        /// 
        /// Returns
        /// -------
        /// Option<Value>
        ///     A JSON serializable primitive.
        
        // Override in implementation
        None
    }

    pub fn calculate_from_orders(&self, orders: &Vec<Order>) -> Option<Value> {
        /// Calculate the statistic value from the given orders.
        /// 
        /// Parameters
        /// ----------
        /// orders : Vec<Order>
        ///     The positions to use for the calculation.
        /// 
        /// Returns
        /// -------
        /// Option<Value>
        ///     A JSON serializable primitive.
        
        // Override in implementation
        None
    }

    pub fn calculate_from_positions(&self, positions: &Vec<Position>) -> Option<Value> {
        /// Calculate the statistic value from the given positions.
        /// 
        /// Parameters
        /// ----------
        /// positions : Vec<Position>
        ///     The positions to use for the calculation.
        /// 
        /// Returns
        /// -------
        /// Option<Value>
        ///     A JSON serializable primitive.
        
        // Override in implementation
        None
    }

    pub fn check_valid_returns(&self, returns: &Vec<(NaiveDateTime, f64)>) -> bool {
        if returns.is_empty() || returns.iter().all(|(_, value)| value.is_nan()) {
            false
        } else {
            true
        }
    }

    pub fn downsample_to_daily_bins(&self, returns: &Vec<(NaiveDateTime, f64)>) -> Vec<(NaiveDateTime, f64)> {
        // This is a simplified version. In Rust, you'd need to implement the resampling logic manually
        // or use a library that provides similar functionality to pandas' resample method.
        returns.iter()
            .filter(|(_, value)| !value.is_nan())
            .fold(std::collections::HashMap::new(), |mut acc, (date, value)| {
                let day = date.date();
                acc.entry(day).or_insert(0.0).add_assign(*value);
                acc
            })
            .into_iter()
            .map(|(date, sum)| (date.and_hms(0, 0, 0), sum))
            .collect()
    }
}