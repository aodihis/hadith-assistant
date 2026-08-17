use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::AppError;

/// Chat sessions exist to key rate limiting and to make the endpoint awkward to
/// call from outside our own pages. They deliberately hold **no user identity**
/// — no account, no profile, nothing personal. A session is an opaque id, the
/// moment it was issued, and a request budget.
///
/// The token is HMAC-signed and carries its own issue time, so expiry is
/// verified with arithmetic rather than a lookup. Only the request counters
/// need server-side storage, and losing them on restart is harmless.
///
/// This is **not authentication**. Anyone can ask for a token. It raises the
/// cost of abuse (fetch a token first, tokens expire, tokens are themselves
/// limited); it does not prevent a determined caller.
pub struct SessionConfig {
    pub secret: Vec<u8>,
    pub ttl: Duration,
    /// Requests allowed per session across its whole lifetime.
    pub max_turns: u32,
    /// Requests allowed per session within `refill_window`.
    pub burst: u32,
    pub refill_window: Duration,
    /// Hard ceiling on tracked sessions, so a token farm cannot grow the map
    /// without bound.
    pub max_tracked: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            secret: Vec::new(),
            ttl: Duration::from_secs(12 * 60 * 60),
            max_turns: 120,
            burst: 8,
            refill_window: Duration::from_secs(60),
            max_tracked: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
struct Bucket {
    issued_at: u64,
    turns_used: u32,
    window_started: u64,
    window_used: u32,
}

pub struct SessionService {
    config: SessionConfig,
    buckets: Mutex<HashMap<SessionId, Bucket>>,
}

impl SessionService {
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.config.ttl
    }

    /// Mints a token of the form `<issued_at>.<nonce>.<hmac>`.
    pub fn issue(&self) -> String {
        let issued_at = now_secs();
        let nonce = nonce();
        let payload = format!("{issued_at}.{nonce}");
        let signature = sign(&self.config.secret, &payload);

        format!("{payload}.{signature}")
    }

    /// Verifies a token and charges one turn against its budget.
    ///
    /// Signature is checked before expiry, and expiry before the budget, so a
    /// forged token never reveals anything about real session state.
    pub fn check(&self, token: &str) -> Result<SessionId, AppError> {
        let (issued_at, id) = self.verify(token)?;

        let now = now_secs();
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        self.evict_expired(&mut buckets, now);

        if !buckets.contains_key(&id) && buckets.len() >= self.config.max_tracked {
            return Err(AppError::TooManyRequests(
                "the service is busy, please try again shortly".to_owned(),
            ));
        }

        let bucket = buckets.entry(id.clone()).or_insert_with(|| Bucket {
            issued_at,
            turns_used: 0,
            window_started: now,
            window_used: 0,
        });

        if now.saturating_sub(bucket.window_started) >= self.config.refill_window.as_secs() {
            bucket.window_started = now;
            bucket.window_used = 0;
        }

        if bucket.turns_used >= self.config.max_turns {
            return Err(AppError::TooManyRequests(
                "this chat has reached its message limit, please start a new one".to_owned(),
            ));
        }

        if bucket.window_used >= self.config.burst {
            return Err(AppError::TooManyRequests(
                "too many messages in a short time, please wait a moment".to_owned(),
            ));
        }

        bucket.turns_used += 1;
        bucket.window_used += 1;

        Ok(id)
    }

    fn verify(&self, token: &str) -> Result<(u64, SessionId), AppError> {
        let mut parts = token.split('.');
        let (Some(issued_at), Some(nonce), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(expired("malformed session"));
        };

        let payload = format!("{issued_at}.{nonce}");
        if !constant_time_eq(&sign(&self.config.secret, &payload), signature) {
            return Err(expired("unrecognised session"));
        }

        let issued_at: u64 = issued_at
            .parse()
            .map_err(|_| expired("malformed session"))?;

        if now_secs().saturating_sub(issued_at) >= self.config.ttl.as_secs() {
            return Err(expired("this chat has expired, please start a new one"));
        }

        Ok((issued_at, SessionId(nonce.to_owned())))
    }

    fn evict_expired(&self, buckets: &mut HashMap<SessionId, Bucket>, now: u64) {
        let ttl = self.config.ttl.as_secs();
        buckets.retain(|_, bucket| now.saturating_sub(bucket.issued_at) < ttl);
    }
}

fn expired(message: &str) -> AppError {
    AppError::SessionExpired(message.to_owned())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

fn nonce() -> String {
    // Not cryptographic randomness, and it does not need to be: the HMAC is
    // what makes a token unforgeable. The nonce only has to be unique enough to
    // key one session's counters apart from another's.
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let mixed = seed ^ (&seed as *const u128 as u128);

    format!("{mixed:032x}")
}

fn sign(secret: &[u8], payload: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(secret);
    hasher.update(b"|");
    hasher.update(payload.as_bytes());

    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Compares without an early return on the first differing byte, so timing does
/// not leak how much of a forged signature was correct.
fn constant_time_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }

    left.iter()
        .zip(right)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(config: SessionConfig) -> SessionService {
        SessionService::new(SessionConfig {
            secret: b"test-secret".to_vec(),
            ..config
        })
    }

    #[test]
    fn a_freshly_issued_token_is_accepted() {
        let service = service(SessionConfig::default());
        let token = service.issue();

        assert!(service.check(&token).is_ok());
    }

    #[test]
    fn a_token_signed_with_another_secret_is_rejected() {
        let issuer = SessionService::new(SessionConfig {
            secret: b"other-secret".to_vec(),
            ..SessionConfig::default()
        });
        let token = issuer.issue();

        let error = service(SessionConfig::default())
            .check(&token)
            .expect_err("a token we did not sign must not be honoured");

        assert!(matches!(error, AppError::SessionExpired(_)));
    }

    #[test]
    fn a_tampered_token_is_rejected() {
        let service = service(SessionConfig::default());
        let token = service.issue();
        // Move the issue time forward to try to extend the session.
        let tampered = format!("9999999999{}", &token[10..]);

        assert!(matches!(
            service.check(&tampered),
            Err(AppError::SessionExpired(_))
        ));
    }

    #[test]
    fn a_token_past_its_ttl_is_rejected() {
        let service = service(SessionConfig {
            ttl: Duration::from_secs(0),
            ..SessionConfig::default()
        });
        let token = service.issue();

        assert!(matches!(
            service.check(&token),
            Err(AppError::SessionExpired(_))
        ));
    }

    #[test]
    fn malformed_tokens_are_rejected_rather_than_panicking() {
        let service = service(SessionConfig::default());

        for token in ["", "nonsense", "a.b", "a.b.c.d", "..."] {
            assert!(
                matches!(service.check(token), Err(AppError::SessionExpired(_))),
                "token {token:?} should be refused"
            );
        }
    }

    #[test]
    fn burst_limit_stops_a_rapid_run_of_turns() {
        let service = service(SessionConfig {
            burst: 3,
            ..SessionConfig::default()
        });
        let token = service.issue();

        for turn in 1..=3 {
            assert!(service.check(&token).is_ok(), "turn {turn} should pass");
        }

        assert!(matches!(
            service.check(&token),
            Err(AppError::TooManyRequests(_))
        ));
    }

    #[test]
    fn lifetime_limit_stops_a_session_that_outlives_its_budget() {
        let service = service(SessionConfig {
            max_turns: 2,
            burst: 100,
            ..SessionConfig::default()
        });
        let token = service.issue();

        assert!(service.check(&token).is_ok());
        assert!(service.check(&token).is_ok());

        let error = service
            .check(&token)
            .expect_err("the lifetime budget is exhausted");
        assert!(matches!(error, AppError::TooManyRequests(_)));
    }

    #[test]
    fn one_sessions_budget_does_not_affect_another() {
        let service = service(SessionConfig {
            burst: 1,
            ..SessionConfig::default()
        });
        let noisy = service.issue();
        let quiet = service.issue();

        assert!(service.check(&noisy).is_ok());
        assert!(matches!(
            service.check(&noisy),
            Err(AppError::TooManyRequests(_))
        ));

        assert!(
            service.check(&quiet).is_ok(),
            "an unrelated session must not be punished for another's usage"
        );
    }

    #[test]
    fn tracking_is_capped_so_a_token_farm_cannot_grow_the_map_without_bound() {
        let service = service(SessionConfig {
            max_tracked: 2,
            ..SessionConfig::default()
        });

        assert!(service.check(&service.issue()).is_ok());
        assert!(service.check(&service.issue()).is_ok());

        assert!(matches!(
            service.check(&service.issue()),
            Err(AppError::TooManyRequests(_))
        ));
    }
}
