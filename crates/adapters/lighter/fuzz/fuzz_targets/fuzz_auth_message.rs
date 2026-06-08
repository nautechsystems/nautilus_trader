#![no_main]

//! Fuzz `hash_auth_message` against arbitrary UTF-8 strings.
//!
//! `hash_auth_message` is reachable from any caller passing an
//! externally-derived message. It MUST never panic and MUST return either a
//! 40-byte digest or a typed `MessageEncoding` error.
//!
//! Inputs that fail UTF-8 are skipped; the production callers only feed
//! ASCII through `auth_token_message`, but fuzzing across arbitrary
//! Unicode catches any unsafe slicing or unchecked decode in the limb-pack
//! path that future callers might exercise.

use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::auth_token::hash_auth_message;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let _ = hash_auth_message(s);
});
