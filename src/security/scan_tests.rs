//! Tests for the secret scanner.

use super::*;

#[test]
fn redact_secrets_replaces_match_keeps_context() {
    // Telemetry redaction replaces the match, keeps the
    // surrounding text, and tags the secret kind — the event
    // stays useful without carrying the credential.
    let out = redact_secrets("error: auth failed for sk-abcdefghijklmnopqrstuvwx retrying");
    assert!(out.contains("[redacted: OpenAI API key]"));
    assert!(!out.contains("sk-"));
    assert!(out.contains("error: auth failed for"));
    assert!(out.contains("retrying"));

    let clean = "a perfectly ordinary error message";
    assert_eq!(redact_secrets(clean), clean);
}

#[test]
fn detects_private_key() {
    let body = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...";
    assert_eq!(scan_content("", body).unwrap().kind, SecretKind::PrivateKey);
}

#[test]
fn detects_github_token() {
    let body = "token: ghp_1234567890abcdefghijklmnopqrstuvwxyz";
    assert_eq!(
        scan_content("", body).unwrap().kind,
        SecretKind::GithubToken
    );
}

#[test]
fn detects_aws_access_key() {
    let body = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
    assert_eq!(
        scan_content("", body).unwrap().kind,
        SecretKind::AwsAccessKey
    );
}

#[test]
fn detects_openai_key() {
    let body = "sk-proj-abcdefghijklmnopqrstuvwx";
    assert_eq!(scan_content("", body).unwrap().kind, SecretKind::OpenAiKey);
}

#[test]
fn detects_anthropic_key() {
    let body = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";
    assert_eq!(
        scan_content("", body).unwrap().kind,
        SecretKind::AnthropicKey
    );
}

#[test]
fn detects_generic_credential_in_quotes() {
    let body = r#""password": "hunter2abc""#;
    assert_eq!(
        scan_content("", body).unwrap().kind,
        SecretKind::GenericCredential
    );
}

/// Positive + negative per scanner pattern — every pattern must
/// both catch its shape and pass its near-miss.
#[test]
fn pattern_table() {
    for (content, kind) in [
        (
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nProc-Type: 4",
            SecretKind::PrivateKey,
        ),
        (
            "tok: github_pat_11ABCDEFG0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            SecretKind::GithubToken,
        ),
        (
            "tok: glpat-\x5ax9Qw8Er7Ty6Ui5Op4As",
            SecretKind::GitlabToken,
        ),
        (
            "key = hf_abcdefghijklmnopqrstuvwxyz01234",
            SecretKind::HuggingFaceToken,
        ),
        (
            "key = AIzaSyD4iE2xY7ZqW9vB1nM3kL5jH8gF6dS4a3x",
            SecretKind::GoogleApiKey,
        ),
        (
            "key = sk_live_\x34eC39HqLyjWDarjtT1zdp7dc",
            SecretKind::StripeKey,
        ),
        (
            "key = rk_live_\x34eC39HqLyjWDarjtT1zdp7dc",
            SecretKind::StripeKey,
        ),
        ("AWS_SESSION=ASIAIOSFODNN7EXAMPLE", SecretKind::AwsAccessKey),
        (
            "jwt: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.sig",
            SecretKind::Jwt,
        ),
        (
            "db: postgres://admin:s3cret@db.internal/x",
            SecretKind::CredentialInUri,
        ),
        ("API_TOKEN = abcdefgh1234", SecretKind::GenericCredential),
    ] {
        let scan = scan_content("", content)
            .unwrap_or_else(|| panic!("expected detection for {content:?}"));
        assert_eq!(scan.kind, kind, "kind mismatch for {content:?}");
    }

    for clean in [
        "-----BEGIN PRIVATE KEY BLOCK-----\nnot a pem",
        "github_pat_short",
        "glpat-tooshort",
        "hf_shorttoken",
        "AIzaShort",
        "sk_live_short",
        "ASIA1234",
        "eyJhbGciOiJIUzI1NiJ9.onlyonesegment",
        "https://example.com/path",
        "MAX_TOKEN_LEN = 4096",
        "API_KEY = \"abc\"",
    ] {
        assert!(
            scan_content("", clean).is_none(),
            "false positive on {clean:?}"
        );
    }
}

/// The scan covers the serialized file — a secret in frontmatter
/// is as committed as one in the body.
#[test]
fn frontmatter_is_scanned() {
    let file = "---\nid: x\ntitle: T\n\ndeploy_token: glpat-\x5ax9Qw8Er7Ty6Ui5Op4As\n---\n\nBody";
    assert_eq!(
        scan_content("", file).unwrap().kind,
        SecretKind::GitlabToken
    );
}

#[test]
fn detects_generic_credential_with_equals() {
    let body = r#"api_key = "mysecret123abc""#;
    assert_eq!(
        scan_content("", body).unwrap().kind,
        SecretKind::GenericCredential
    );
}

#[test]
fn detects_in_title() {
    let title = "Key: ghp_1234567890abcdefghijklmnopqrstuvwxyz";
    assert_eq!(
        scan_content(title, "").unwrap().kind,
        SecretKind::GithubToken
    );
}

#[test]
fn no_false_positive_on_short_values() {
    // "password: \"abc\"" is only 3 chars — below the 8-char minimum.
    let body = r#"password = "abc""#;
    assert!(scan_content("", body).is_none());
}

#[test]
fn no_false_positive_on_discussion_text() {
    let body = "When using API keys, store them in environment variables, not in code.";
    assert!(scan_content("", body).is_none());
}

#[test]
fn no_false_positive_on_uuids() {
    let body = "Entity ID: 550e8400-e29b-41d4-a716-446655440000";
    assert!(scan_content("", body).is_none());
}

#[test]
fn no_false_positive_on_hashes() {
    let body = "SHA-256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert!(scan_content("", body).is_none());
}

#[test]
fn no_false_positive_on_base64_discussion() {
    let body = "Encode the payload as base64 before sending it to the API endpoint.";
    assert!(scan_content("", body).is_none());
}

#[test]
fn preview_is_truncated() {
    let body = "ghp_1234567890abcdefghijklmnopqrstuvwxyz";
    let result = scan_content("", body).unwrap();
    assert!(result.preview.chars().count() <= 4);
    assert_eq!(result.preview, "ghp_");
}
