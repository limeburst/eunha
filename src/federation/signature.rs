//! Re-exports feder-core's HTTP Signature implementation.
//!
//! Also provides an adapter to convert axum's `HeaderMap` into the flat
//! slice format expected by feder-core.

pub use feder_core::signature::{key_id_from_header, sign_request, verify_request, SignedHeaders};

/// Convert an axum `HeaderMap` to a `Vec<(String, String)>` for use with
/// [`verify_request`].
pub fn headers_to_vec(map: &axum::http::HeaderMap) -> Vec<(String, String)> {
    map.iter()
        .filter_map(|(k, v)| {
            v.to_str().ok().map(|val| (k.as_str().to_lowercase(), val.to_string()))
        })
        .collect()
}
