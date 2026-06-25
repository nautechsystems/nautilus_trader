use super::{PyStubType, TypeInfo};

impl PyStubType for rust_decimal::Decimal {
    fn type_output() -> TypeInfo {
        TypeInfo::with_module("decimal.Decimal", "decimal".into())
    }
}
