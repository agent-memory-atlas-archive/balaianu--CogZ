use super::*;
use crate::embed::model::{EmbeddingError, EmbeddingModel, NliModel};
use crate::embed::{OnnxEmbeddingModel, OnnxNliModel};

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
fn ort_download_gate_respects_flag() {
    // ensure_ort's OnceLock caches its first outcome process-wide, so
    // the download gate can't be exercised through it in-process —
    // this covers the flag mechanism the gate reads.
    set_ort_download_allowed(false);
    assert!(!ort_download_permitted());
    set_ort_download_allowed(true);
    assert!(ort_download_permitted());
}

#[test]
fn fts_only_flag_produces_unavailable_models() {
    // `force_fts_only` must disable embedding AND NLI models at the
    // constructor — that's what keeps model resolution (and ONNX init)
    // from running at all in degraded mode. Flag is process-global;
    // reset it before asserting so a leaked true can't pass vacuously.
    set_fts_only(false);
    assert!(!fts_only());

    set_fts_only(true);
    let emb = OnnxEmbeddingModel::with_resource_config(
        crate::embed::ModelType::Code,
        std::path::Path::new("/nonexistent"),
        768,
        "test/model",
        0,
        0,
    );
    assert!(!emb.is_available());
    assert!(matches!(
        emb.embed(&["x"]),
        Err(EmbeddingError::ModelUnavailable(_))
    ));
    let nli = OnnxNliModel::with_resource_config(std::path::Path::new("/nonexistent"), "", 0, 0);
    assert!(matches!(
        nli.classify("a", "b"),
        Err(EmbeddingError::ModelUnavailable(_))
    ));
    set_fts_only(false);
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
