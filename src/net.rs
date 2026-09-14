//! HTTP agents with bounded timeouts for all outbound requests.

use std::time::Duration;

/// Agent for small API calls — the whole call is bounded.
pub fn api_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_per_call(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// Agent for binary downloads. Bounds connect and first-byte waits —
/// the phases that hang on dead hosts — but not the body, so slow
/// connections can still finish large downloads.
pub fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into()
}
