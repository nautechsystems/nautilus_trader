// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use nautilus_core::MUTEX_POISONED;

use super::types::SignerId;
use crate::http::error::{Error, Result};

/// Time-based nonce in Unix milliseconds for Hyperliquid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeNonce(pub i128);

impl TimeNonce {
    /// Create from Unix milliseconds.
    pub fn from_millis(ms: i128) -> Self {
        Self(ms)
    }

    /// Get as milliseconds.
    pub fn as_millis(self) -> i128 {
        self.0
    }

    /// Current time in milliseconds.
    ///
    /// # Panics
    ///
    /// Panics if the system time is before the Unix epoch.
    pub fn now_millis() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        Self::from_millis(now.as_millis() as i128)
    }
}

impl std::fmt::Display for TimeNonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Nonce policy configuration for Hyperliquid.
#[derive(Debug, Clone)]
pub struct NoncePolicy {
    pub past_ms: i64,
    pub future_ms: i64,
    pub keep_last_n: usize,
}

impl NoncePolicy {
    pub fn new(past_ms: i64, future_ms: i64, keep_last_n: usize) -> Self {
        Self {
            past_ms,
            future_ms,
            keep_last_n,
        }
    }
}

impl Default for NoncePolicy {
    fn default() -> Self {
        Self {
            past_ms: 2 * 24 * 60 * 60 * 1000,
            future_ms: 24 * 60 * 60 * 1000,
            keep_last_n: 100,
        }
    }
}

/// Error types for Hyperliquid nonce validation.
#[derive(Debug, thiserror::Error)]
pub enum NonceError {
    #[error("Nonce too old: {nonce} is before window start {window_start}")]
    TooOld {
        nonce: TimeNonce,
        window_start: TimeNonce,
    },

    #[error("Nonce too new: {nonce} is after window end {window_end}")]
    TooNew {
        nonce: TimeNonce,
        window_end: TimeNonce,
    },

    #[error("Nonce already used: {nonce}")]
    AlreadyUsed { nonce: TimeNonce },

    #[error("Nonce must be greater than minimum: {nonce} <= {min_nonce}")]
    NotMonotonic {
        nonce: TimeNonce,
        min_nonce: TimeNonce,
    },
}

/// Per-signer nonce state for Hyperliquid.
#[derive(Debug)]
struct SignerState {
    next_nonce: i128,
    used_nonces: VecDeque<TimeNonce>,
    max_used: usize,
}

impl SignerState {
    fn new(initial_nonce: i128, max_used: usize) -> Self {
        Self {
            next_nonce: initial_nonce,
            used_nonces: VecDeque::with_capacity(max_used),
            max_used,
        }
    }

    fn next_nonce(&mut self) -> TimeNonce {
        // Always ensure we're at least at current time
        let now = TimeNonce::now_millis().0;
        self.next_nonce = self.next_nonce.max(now);

        // Use and increment atomically to prevent reuse
        let nonce = TimeNonce::from_millis(self.next_nonce);
        self.next_nonce += 1;

        self.used_nonces.push_back(nonce);
        if self.used_nonces.len() > self.max_used {
            self.used_nonces.pop_front();
        }

        nonce
    }

    fn validate_local(
        &self,
        nonce: TimeNonce,
        _policy: &NoncePolicy,
    ) -> std::result::Result<(), NonceError> {
        // Check for replay attacks
        if self.used_nonces.contains(&nonce) {
            return Err(NonceError::AlreadyUsed { nonce });
        }

        // Check for monotonicity (nonce must be greater than the oldest tracked nonce)
        if let Some(&min_used) = self.used_nonces.front()
            && nonce.0 <= min_used.0
        {
            return Err(NonceError::NotMonotonic {
                nonce,
                min_nonce: min_used,
            });
        }

        Ok(())
    }

    fn fast_forward_to(&mut self, now_ms: i128) {
        if now_ms > self.next_nonce {
            self.next_nonce = now_ms;
        }
    }
}

/// Thread-safe nonce manager for Hyperliquid signers.
#[derive(Debug)]
pub struct NonceManager {
    policy: NoncePolicy,
    signer_states: Arc<Mutex<HashMap<SignerId, SignerState>>>,
}

impl NonceManager {
    pub fn new() -> Self {
        Self {
            policy: NoncePolicy::default(),
            signer_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_policy(policy: NoncePolicy) -> Self {
        Self {
            policy,
            signer_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Generate the next nonce for a given signer.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn next(&self, signer: SignerId) -> Result<TimeNonce> {
        let mut states = self.signer_states.lock().expect(MUTEX_POISONED);
        let state = states.entry(signer).or_insert_with(|| {
            SignerState::new(TimeNonce::now_millis().0, self.policy.keep_last_n)
        });
        Ok(state.next_nonce())
    }

    /// Fast-forward all signers to a given time.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn fast_forward_to(&self, now_ms: i128) {
        let mut states = self.signer_states.lock().expect(MUTEX_POISONED);
        for state in states.values_mut() {
            state.fast_forward_to(now_ms);
        }
    }

    /// Validate a nonce locally without network calls.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn validate_local(&self, signer: SignerId, nonce: TimeNonce) -> Result<()> {
        let states = self.signer_states.lock().expect(MUTEX_POISONED);

        // Always validate time window, even for new signers
        let now_ms = TimeNonce::now_millis().0;
        let window_start = now_ms - self.policy.past_ms as i128;
        let window_end = now_ms + self.policy.future_ms as i128;

        if nonce.0 < window_start {
            return Err(Error::nonce_window(format!(
                "Nonce too old: {} is before window start {}",
                nonce,
                TimeNonce::from_millis(window_start)
            )));
        }

        if nonce.0 > window_end {
            return Err(Error::nonce_window(format!(
                "Nonce too new: {} is after window end {}",
                nonce,
                TimeNonce::from_millis(window_end)
            )));
        }

        // If signer state exists, validate against used nonces and monotonicity
        if let Some(state) = states.get(&signer) {
            state
                .validate_local(nonce, &self.policy)
                .map_err(|e| Error::nonce_window(e.to_string()))?;
        }

        Ok(())
    }

    pub fn policy(&self) -> &NoncePolicy {
        &self.policy
    }
}

impl Default for NonceManager {
    fn default() -> Self {
        Self::new()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::thread;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_time_nonce_creation() {
        let nonce_ms = TimeNonce::from_millis(1640995200000);
        assert_eq!(nonce_ms.as_millis(), 1640995200000);
    }

    #[rstest]
    fn test_nonce_monotonicity() {
        let manager = NonceManager::new();
        let signer = SignerId::from("test_signer");

        let nonce1 = manager.next(signer.clone()).unwrap();
        let nonce2 = manager.next(signer.clone()).unwrap();
        let nonce3 = manager.next(signer).unwrap();

        assert!(nonce2 > nonce1);
        assert!(nonce3 > nonce2);
    }

    #[rstest]
    fn test_nonce_window_validation() {
        let manager = NonceManager::new();
        let signer = SignerId::from("test_signer");

        let valid_nonce = TimeNonce::now_millis();
        assert!(manager.validate_local(signer.clone(), valid_nonce).is_ok());

        let old_nonce = TimeNonce::from_millis(TimeNonce::now_millis().0 - 3 * 24 * 60 * 60 * 1000);
        assert!(manager.validate_local(signer.clone(), old_nonce).is_err());

        let future_nonce =
            TimeNonce::from_millis(TimeNonce::now_millis().0 + 2 * 24 * 60 * 60 * 1000);
        assert!(manager.validate_local(signer, future_nonce).is_err());
    }

    #[rstest]
    fn test_nonce_deduplication() {
        let manager = NonceManager::new();
        let signer = SignerId::from("test_signer");

        let nonce = manager.next(signer.clone()).unwrap();
        assert!(manager.validate_local(signer, nonce).is_err());
    }

    #[rstest]
    fn test_fast_forward() {
        let manager = NonceManager::new();
        let signer = SignerId::from("test_signer");

        let nonce1 = manager.next(signer.clone()).unwrap();

        let future_time = TimeNonce::now_millis().0 + 10_000;
        manager.fast_forward_to(future_time);

        let nonce2 = manager.next(signer).unwrap();
        assert!(nonce2.0 >= future_time);
        assert!(nonce2 > nonce1); // Ensure the new nonce is greater than the old one
    }

    #[rstest]
    fn test_concurrent_nonce_generation() {
        let manager = Arc::new(NonceManager::new());
        let signer = SignerId::from("concurrent_signer");

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let manager = Arc::clone(&manager);
                let signer = signer.clone();
                thread::spawn(move || manager.next(signer).unwrap())
            })
            .collect();

        let mut nonces: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        nonces.sort();

        for i in 1..nonces.len() {
            assert!(nonces[i] > nonces[i - 1]);
        }
    }

    #[rstest]
    fn test_custom_policy() {
        let policy = NoncePolicy::new(1000, 2000, 50);
        let manager = NonceManager::with_policy(policy);

        assert_eq!(manager.policy().past_ms, 1000);
        assert_eq!(manager.policy().future_ms, 2000);
        assert_eq!(manager.policy().keep_last_n, 50);
    }
}
