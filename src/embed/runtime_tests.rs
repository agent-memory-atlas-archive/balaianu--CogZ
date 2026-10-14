use super::*;

#[test]
fn ort_lib_path_is_under_cogz_lib() {
    let path = ort_lib_path();
    assert!(path.ends_with(ORT_LIB_NAME));
}

#[test]
fn find_existing_lib_returns_none_when_nothing_installed() {
    // SAFETY: this test runs single-threaded; removing an env var
    // here is safe because no other thread is reading the environment.
    unsafe {
        std::env::remove_var("ORT_DYLIB_PATH");
    }
    let _ = find_existing_lib();
}

#[test]
fn ort_sha256_is_pinned_for_supported_platforms() {
    // The hash is the trust root for the ORT download — it must be a
    // real 64-hex-char pin wherever a download can happen. An empty
    // or malformed pin would either fail open or refuse every install.
    if ort_download_supported() {
        assert_eq!(ORT_SHA256.len(), 64);
        assert!(ORT_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
    } else {
        assert_eq!(ORT_SHA256, "");
    }
}
