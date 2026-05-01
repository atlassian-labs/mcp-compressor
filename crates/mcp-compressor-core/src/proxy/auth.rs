//! Bearer-token authentication for the generic tool proxy.
//!
//! # Design
//!
//! `SessionToken` wraps a 64-character hex string (32 random bytes).  A fresh
//! token is minted at proxy startup and embedded into every generated client
//! artifact.  Stale artifacts from previous sessions simply fail to
//! authenticate.
//!
//! Token verification uses a constant-time comparison (byte-wise XOR fold) to
//! prevent timing side-channels — matching the Python `hmac.compare_digest`
//! behaviour.
//!
//! # Wire format
//!
//! Clients must send `Authorization: Bearer <token>` in every `POST /exec`
//! request.  The `verify` method accepts the raw header value (everything
//! after the colon-space).

use rand::Rng;

/// A session-scoped bearer token.
///
/// ## Lifetime
///
/// One `SessionToken` is created per proxy-server invocation.  It is printed
/// to stderr (informational) and embedded at generation time into every client
/// artifact produced for that session.
#[derive(Debug, Clone)]
pub struct SessionToken(String);

impl SessionToken {
    /// Generate a new cryptographically random 64-character hex token.
    ///
    /// Internally draws 32 random bytes from the OS RNG and hex-encodes them,
    /// yielding a string that is unique with overwhelming probability.
    pub fn generate() -> Self {
        todo!()
    }

    /// Return the raw hex token string (without the `Bearer` prefix).
    pub fn value(&self) -> &str {
        todo!()
    }

    /// Verify an `Authorization` header value in constant time.
    ///
    /// The `header` argument is the **full** header value, e.g.
    /// `"Bearer a3f7..."`.  Returns `true` only when the header matches
    /// `"Bearer <self.value()>"` exactly (case-sensitive on the prefix).
    ///
    /// Uses XOR-fold over bytes to avoid early-exit timing leaks.
    pub fn verify(&self, header: &str) -> bool {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Token generation
    // ------------------------------------------------------------------

    /// A generated token is non-empty.
    #[test]
    fn generate_non_empty() {
        let token = SessionToken::generate();
        assert!(!token.value().is_empty());
    }

    /// The generated token is exactly 64 characters (32 bytes, hex-encoded).
    #[test]
    fn generate_length_64() {
        let token = SessionToken::generate();
        assert_eq!(token.value().len(), 64, "expected 64-char hex string, got {:?}", token.value());
    }

    /// The generated token contains only valid lowercase hex characters.
    #[test]
    fn generate_is_lowercase_hex() {
        let token = SessionToken::generate();
        assert!(
            token.value().chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "token is not lowercase hex: {:?}",
            token.value(),
        );
    }

    /// Two independently generated tokens are different (uniqueness).
    ///
    /// The probability of a collision for 256-bit tokens is negligible; a
    /// failure here would strongly indicate a broken RNG.
    #[test]
    fn generate_produces_unique_tokens() {
        let t1 = SessionToken::generate();
        let t2 = SessionToken::generate();
        assert_ne!(t1.value(), t2.value(), "two tokens must not be identical");
    }

    /// value() returns the raw hex string without any prefix.
    #[test]
    fn value_returns_raw_hex() {
        let token = SessionToken::generate();
        let val = token.value();
        // No "Bearer" prefix
        assert!(!val.starts_with("Bearer "));
        // No whitespace
        assert!(!val.contains(' '));
    }

    // ------------------------------------------------------------------
    // verify — successful authentication
    // ------------------------------------------------------------------

    /// A correct "Bearer <token>" header is accepted.
    #[test]
    fn verify_correct_bearer_header() {
        let token = SessionToken::generate();
        let header = format!("Bearer {}", token.value());
        assert!(token.verify(&header), "correct header must be accepted");
    }

    // ------------------------------------------------------------------
    // verify — rejection cases
    // ------------------------------------------------------------------

    /// An empty string is rejected.
    #[test]
    fn verify_empty_string_rejected() {
        let token = SessionToken::generate();
        assert!(!token.verify(""), "empty header must be rejected");
    }

    /// A random wrong token is rejected.
    #[test]
    fn verify_wrong_token_rejected() {
        let token = SessionToken::generate();
        assert!(!token.verify("Bearer wrongtoken0000000000000000000000000000000000000000000000000000"));
    }

    /// The raw token without the "Bearer " prefix is rejected.
    #[test]
    fn verify_raw_token_without_prefix_rejected() {
        let token = SessionToken::generate();
        assert!(!token.verify(token.value()), "bare token without 'Bearer ' must be rejected");
    }

    /// A header with a lowercase "bearer" prefix is rejected (case-sensitive).
    #[test]
    fn verify_lowercase_bearer_rejected() {
        let token = SessionToken::generate();
        let header = format!("bearer {}", token.value());
        assert!(!token.verify(&header), "lowercase 'bearer' prefix must be rejected");
    }

    /// "Bearer" with no token value after it is rejected.
    #[test]
    fn verify_bearer_with_no_token_rejected() {
        let token = SessionToken::generate();
        assert!(!token.verify("Bearer"), "bare 'Bearer' with no value must be rejected");
    }

    /// "Bearer " (trailing space, no token) is rejected.
    #[test]
    fn verify_bearer_trailing_space_rejected() {
        let token = SessionToken::generate();
        assert!(!token.verify("Bearer "), "'Bearer ' with empty token must be rejected");
    }

    /// A token from a *different* session is rejected.
    #[test]
    fn verify_different_session_token_rejected() {
        let token1 = SessionToken::generate();
        let token2 = SessionToken::generate();
        let header = format!("Bearer {}", token2.value());
        assert!(!token1.verify(&header), "different session token must be rejected");
    }

    /// A token with one character flipped is rejected.
    ///
    /// This exercises the constant-time path where lengths match but content
    /// differs.
    #[test]
    fn verify_single_bit_flip_rejected() {
        let token = SessionToken::generate();
        // Flip the first hex digit: replace with the next character or wrap.
        let mut bad = format!("Bearer {}", token.value());
        // The 7th character (index 7) is the start of the token hex string.
        let bytes = unsafe { bad.as_bytes_mut() };
        bytes[7] ^= 1; // mutate one byte of the token
        let bad = String::from_utf8(bytes.to_vec()).unwrap_or_default();
        assert!(!token.verify(&bad), "one-bit-flipped token must be rejected");
    }

    // ------------------------------------------------------------------
    // verify — constant-time property (structural)
    // ------------------------------------------------------------------

    /// verify always examines all bytes, even when the prefix is wrong.
    ///
    /// This is a structural test: we confirm verify returns false for a string
    /// that has the right length but wrong prefix, and also for one that has
    /// a completely wrong length.  The actual constant-time guarantee is
    /// enforced by implementation (XOR fold), not easily measurable in unit
    /// tests, but is documented here as a specification requirement.
    #[test]
    fn verify_does_not_short_circuit_on_wrong_prefix() {
        let token = SessionToken::generate();
        // Build a header of the correct *total* length but with "Hearer " as prefix
        let fake_header = format!("Hearer {}", token.value());
        // Length is same as the correct "Bearer <token>" — must still be rejected
        assert_eq!(fake_header.len(), format!("Bearer {}", token.value()).len());
        assert!(!token.verify(&fake_header));
    }
}
