//! Secret pattern scanner.
//!
//! Detects high-confidence secret patterns in entity content before
//! it's written to disk. Returns the matched pattern type so the
//! caller can report what was found.
//!
//! Design choices:
//! - Reject, don't redact. Redaction can mangle content in subtle
//!   ways and the agent should rewrite without the secret.
//! - High-confidence patterns only. False positives are disruptive
//!   (the write is rejected), so each pattern must be specific enough
//!   that it rarely matches non-secret text.
//! - No entropy-based detection. High-entropy string detection has
//!   too many false positives in code-aware knowledge bases that
//!   legitimately discuss hashes, UUIDs, and encoded data.

use std::sync::LazyLock;

/// A detected secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanResult {
    /// What kind of secret was detected.
    pub kind: SecretKind,
    /// The first few characters of the match (for error reporting,
    /// never the full secret).
    pub preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    PrivateKey,
    GithubToken,
    AwsAccessKey,
    SlackToken,
    OpenAiKey,
    AnthropicKey,
    GenericCredential,
    GitlabToken,
    HuggingFaceToken,
    GoogleApiKey,
    StripeKey,
    Jwt,
    CredentialInUri,
}

impl std::fmt::Display for SecretKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrivateKey => write!(f, "private key"),
            Self::GithubToken => write!(f, "GitHub token"),
            Self::AwsAccessKey => write!(f, "AWS access key"),
            Self::SlackToken => write!(f, "Slack token"),
            Self::OpenAiKey => write!(f, "OpenAI API key"),
            Self::AnthropicKey => write!(f, "Anthropic API key"),
            Self::GenericCredential => write!(f, "credential assignment"),
            Self::GitlabToken => write!(f, "GitLab token"),
            Self::HuggingFaceToken => write!(f, "HuggingFace token"),
            Self::GoogleApiKey => write!(f, "Google API key"),
            Self::StripeKey => write!(f, "Stripe live key"),
            Self::Jwt => write!(f, "JWT"),
            Self::CredentialInUri => write!(f, "credential in URI"),
        }
    }
}

struct Pattern {
    kind: SecretKind,
    regex: regex::Regex,
}

static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    vec![
        // PEM private keys (RSA, EC, OpenSSH, PGP, etc.)
        Pattern {
            kind: SecretKind::PrivateKey,
            regex: regex::Regex::new(
                r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY-----",
            )
            .expect("valid regex"),
        },
        // GitHub tokens: ghp_, gho_, ghu_, ghs_, ghr_ + 36+ chars
        Pattern {
            kind: SecretKind::GithubToken,
            regex: regex::Regex::new(r"gh[pousr]_[A-Za-z0-9]{36,}").expect("valid regex"),
        },
        // AWS access key IDs: AKIA + 16 chars
        Pattern {
            kind: SecretKind::AwsAccessKey,
            regex: regex::Regex::new(r"AKIA[0-9A-Z]{16}").expect("valid regex"),
        },
        // Slack tokens: xox[baprs]- + 10+ chars
        Pattern {
            kind: SecretKind::SlackToken,
            regex: regex::Regex::new(r"xox[baprs]-[A-Za-z0-9-]{10,}").expect("valid regex"),
        },
        // OpenAI API keys: sk- + 20+ chars (project keys sk-proj-, classic sk-)
        Pattern {
            kind: SecretKind::OpenAiKey,
            regex: regex::Regex::new(r"sk-(?:proj-)?[A-Za-z0-9]{20,}").expect("valid regex"),
        },
        // Anthropic API keys: sk-ant- + 30+ chars
        Pattern {
            kind: SecretKind::AnthropicKey,
            regex: regex::Regex::new(r"sk-ant-[A-Za-z0-9_-]{30,}").expect("valid regex"),
        },
        // Generic credential assignment:
        // (password|passwd|pwd|secret|api_key|apikey|api-key|token|access_key)
        // optionally quoted, followed by = or : then a quoted string of 8+ chars.
        // Handles JSON ("password": "x"), YAML (password: "x"), and env (api_key="x").
        Pattern {
            kind: SecretKind::GenericCredential,
            regex: regex::Regex::new(
                r#"(?i)(?:password|passwd|pwd|secret|api[_-]?key|token|access[_-]?key)["']?\s*[:=]\s*["'][^"']{8,}["']"#,
            )
            .expect("valid regex"),
        },
        // GitHub fine-grained PATs: github_pat_ + 80+ chars
        Pattern {
            kind: SecretKind::GithubToken,
            regex: regex::Regex::new(r"github_pat_[A-Za-z0-9_]{80,}").expect("valid regex"),
        },
        // GitLab PATs: glpat- + 20+ chars
        Pattern {
            kind: SecretKind::GitlabToken,
            regex: regex::Regex::new(r"glpat-[A-Za-z0-9_-]{20,}").expect("valid regex"),
        },
        // HuggingFace tokens: hf_ + 30+ alphanumerics
        Pattern {
            kind: SecretKind::HuggingFaceToken,
            regex: regex::Regex::new(r"hf_[A-Za-z0-9]{30,}").expect("valid regex"),
        },
        // Google API keys: AIza + 35 chars
        Pattern {
            kind: SecretKind::GoogleApiKey,
            regex: regex::Regex::new(r"AIza[0-9A-Za-z_-]{35}").expect("valid regex"),
        },
        // Stripe live keys: sk_live_/rk_live_ + 20+ chars
        Pattern {
            kind: SecretKind::StripeKey,
            regex: regex::Regex::new(r"(?:sk|rk)_live_[A-Za-z0-9]{20,}").expect("valid regex"),
        },
        // AWS session/temporary key IDs: ASIA + 16 chars
        Pattern {
            kind: SecretKind::AwsAccessKey,
            regex: regex::Regex::new(r"ASIA[0-9A-Z]{16}").expect("valid regex"),
        },
        // JWTs: header.payload. — a bearer token is a credential even
        // when its signature segment is missing.
        Pattern {
            kind: SecretKind::Jwt,
            regex: regex::Regex::new(r"eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.").expect("valid regex"),
        },
        // Credentials embedded in URIs: scheme://user:pass@host
        Pattern {
            kind: SecretKind::CredentialInUri,
            regex: regex::Regex::new(r"[a-z]+://[^\s:/]+:[^\s@]+@").expect("valid regex"),
        },
        // Encrypted PEM blocks — the ciphertext is sensitive even
        // without the passphrase.
        Pattern {
            kind: SecretKind::PrivateKey,
            regex: regex::Regex::new(r"-----BEGIN ENCRYPTED PRIVATE KEY-----").expect("valid regex"),
        },
        // Env-file-style assignments with unquoted values: API_KEY=x
        // at line start. The value must start and stay unquoted —
        // [^\s"'] keeps KEY = "x" in the quoted-pattern lane instead.
        Pattern {
            kind: SecretKind::GenericCredential,
            regex: regex::Regex::new(
                r#"(?m)^\s*[A-Z0-9_]*(?:KEY|TOKEN|SECRET|PASSWORD)\s*=\s*[^\s"']{8,}"#,
            )
            .expect("valid regex"),
        },
    ]
});

/// Scan content for secret patterns. Returns the first match, if any.
///
/// Scans both the title and body. The preview in the result is
/// truncated to 4 characters — just the identifying prefix, not
/// enough to expose the secret body. The `kind` field already
/// identifies the pattern type.
pub fn scan_content(title: &str, body: &str) -> Option<ScanResult> {
    let combined = format!("{}\n{}", title, body);
    for pattern in PATTERNS.iter() {
        if let Some(m) = pattern.regex.find(&combined) {
            let preview = m.as_str().chars().take(4).collect();
            return Some(ScanResult {
                kind: pattern.kind,
                preview,
            });
        }
    }
    None
}

/// Replace every secret-pattern match with `[redacted: kind]`.
///
/// Entity writes reject on `scan_content` — the agent rewrites without
/// the secret. Telemetry can't reject (the hook must still record the
/// event), so stored payload text is redacted instead.
pub fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for pattern in PATTERNS.iter() {
        out = pattern
            .regex
            .replace_all(&out, format!("[redacted: {}]", pattern.kind))
            .into_owned();
    }
    out
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
