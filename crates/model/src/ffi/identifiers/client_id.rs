use std::ffi::c_char;

use nautilus_core::ffi::string::cstr_as_str;

use crate::identifiers::ClientId;

/// Returns a Nautilus identifier from C string pointer.
///
/// # Safety
///
/// Assumes `ptr` is a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn client_id_new(ptr: *const c_char) -> ClientId {
    let value = unsafe { cstr_as_str(ptr) };
    ClientId::from(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn client_id_hash(id: &ClientId) -> u64 {
    id.inner().precomputed_hash()
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use rstest::rstest;

    use super::*;
    use crate::identifiers::stubs::*;

    #[rstest]
    fn test_client_id_to_cstr_c() {
        let id = ClientId::from("BINANCE");
        let c_string = id.inner().as_char_ptr();
        let rust_string = unsafe { CStr::from_ptr(c_string) }.to_str().unwrap();
        assert_eq!(rust_string, "BINANCE");
    }

    #[rstest]
    fn test_client_id_hash_c() {
        let id1 = client_id_binance();
        let id2 = client_id_binance();
        let id3 = client_id_dydx();
        assert_eq!(client_id_hash(&id1), client_id_hash(&id2));
        assert_ne!(client_id_hash(&id1), client_id_hash(&id3));
    }
}
