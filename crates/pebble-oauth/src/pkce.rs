use oauth2::{CsrfToken, PkceCodeChallenge, PkceCodeVerifier};

/// Holds the PKCE verifier and CSRF token needed to complete the OAuth flow.
pub struct PkceState {
    pub verifier: PkceCodeVerifier,
    pub csrf_token: CsrfToken,
}

impl PkceState {
    pub fn into_secrets(self) -> (String, String) {
        (
            self.verifier.secret().to_owned(),
            self.csrf_token.secret().to_owned(),
        )
    }

    pub fn from_secrets(verifier: String, csrf_token: String) -> Self {
        Self {
            verifier: PkceCodeVerifier::new(verifier),
            csrf_token: CsrfToken::new(csrf_token),
        }
    }
}

/// Generate a new random PKCE challenge/verifier pair (SHA-256).
pub fn generate_pkce() -> (PkceCodeChallenge, PkceCodeVerifier) {
    PkceCodeChallenge::new_random_sha256()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_verifier_are_non_empty() {
        let (challenge, verifier) = generate_pkce();
        // The challenge method should be S256
        assert_eq!(challenge.method().as_str(), "S256");
        // Verifier secret must not be empty
        assert!(!verifier.secret().is_empty());
    }

    #[test]
    fn pkce_generates_unique_values() {
        let (_, v1) = generate_pkce();
        let (_, v2) = generate_pkce();
        assert_ne!(v1.secret(), v2.secret());
    }
}
