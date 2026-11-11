// The client's side of the runner's challenge auth (docs/protocol.md,
// "Auth"), the same HMAC as runner/src/rpc-auth.ts. The token never goes
// over the socket and never reaches the web view: only proofs do.
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use subtle::ConstantTimeEq;

pub const AUTH_CONTEXT: &str = "threadbear-rpc-auth-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Client,
    Runner,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Client => "client",
            Role::Runner => "runner",
        }
    }
}

/// 32 random bytes, lowercase hex.
pub fn new_nonce() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn is_nonce(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// HMAC-SHA256, keyed with the token, of the role, both nonces and which
/// server and port the proof is for. `server` is "primary" for a runner's
/// main server, the only kind this client talks to.
pub fn proof(token: &str, role: Role, runner_nonce: &str, client_nonce: &str, server: &str, port: u16) -> String {
    let text = format!("{AUTH_CONTEXT} {}\n{runner_nonce}\n{client_nonce}\n{server} {port}", role.as_str());
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC takes any key length");
    mac.update(text.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn proof_matches(expected: &str, candidate: &str) -> bool {
    expected.len() == candidate.len() && bool::from(expected.as_bytes().ct_eq(candidate.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Worked out with runner/src/rpc-auth.ts's authProof (see
    // runner/test/unit/desktop-auth-vector.test.ts, which checks the same numbers), so
    // the two sides can't drift apart.
    const TOKEN: &str = "test-token";
    const RUNNER_NONCE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const CLIENT_NONCE: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    #[test]
    fn matches_the_runner_for_both_roles() {
        let client = proof(TOKEN, Role::Client, RUNNER_NONCE, CLIENT_NONCE, "primary", 8797);
        let runner = proof(TOKEN, Role::Runner, RUNNER_NONCE, CLIENT_NONCE, "primary", 8797);
        assert_eq!(client, include_str!("../../test/auth-vector-client.txt").trim());
        assert_eq!(runner, include_str!("../../test/auth-vector-runner.txt").trim());
        assert_ne!(client, runner);
    }

    #[test]
    fn port_and_server_change_the_proof() {
        let a = proof(TOKEN, Role::Client, RUNNER_NONCE, CLIENT_NONCE, "primary", 8797);
        assert_ne!(a, proof(TOKEN, Role::Client, RUNNER_NONCE, CLIENT_NONCE, "primary", 8798));
        assert_ne!(a, proof(TOKEN, Role::Client, RUNNER_NONCE, CLIENT_NONCE, "drain", 8797));
    }

    #[test]
    fn nonces() {
        let n = new_nonce();
        assert!(is_nonce(&n));
        assert_ne!(n, new_nonce());
        assert!(!is_nonce("ABCD"));
        assert!(!is_nonce(&"G".repeat(64)));
    }

    #[test]
    fn compares_proofs() {
        assert!(proof_matches("abc", "abc"));
        assert!(!proof_matches("abc", "abd"));
        assert!(!proof_matches("abc", "abcd"));
    }
}
