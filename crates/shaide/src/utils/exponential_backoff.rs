use std::{collections::HashMap, ops::Range, sync::Arc, time::Duration};

use fastrand::Rng;
use futures::lock::Mutex;
use tokio::time::{Instant, sleep_until};
use tracing::debug;

/// Highest exponent used when calculating a model's backoff delay.
///
/// Once this value is reached, subsequent rate-limit responses continue using
/// the maximum delay instead of increasing it further.
const BACKOFF_MAX_EXPONENT: u32 = 5;

/// Base of the exponential delay calculation.
///
/// The delay before jitter is `BACKOFF_FACTOR.pow(current_attempt) * 100`
/// milliseconds.
const BACKOFF_FACTOR: u64 = 2;

/// Inclusive lower bound, in milliseconds, for the random delay added to each
/// waiter after a model's shared backoff deadline.
const BACKOFF_JITTER_MS_MIN: u64 = 0;

/// Exclusive upper bound, in milliseconds, for the random delay added to each
/// waiter after a model's shared backoff deadline.
const BACKOFF_JITTER_MS_MAX: u64 = 200;

struct BackoffBucketAttempt {
    deadline: Instant,
    current_attempt: u32,
}

impl Default for BackoffBucketAttempt {
    fn default() -> Self {
        Self {
            deadline: Instant::now(),
            current_attempt: 0,
        }
    }
}

impl BackoffBucketAttempt {
    fn increase_next_deadline(&mut self, factor: u64, max_attempt: u32) -> Instant {
        if self.current_attempt < max_attempt {
            self.current_attempt += 1;
        }
        // NOTE: we calculate the delay in miliseconds with (factor^current_attempt) * 100. The 100
        // here is not used to convert betwean different measurements
        let millis = factor.pow(self.current_attempt) * 100;
        debug!(
            attempt = self.current_attempt,
            sleep_ms = millis,
            "Increasing backoff attempt"
        );
        Instant::now() + Duration::from_millis(millis)
    }

    fn clear_backoff(&mut self) {
        self.current_attempt = 0;
    }
}

pub struct ExponentialBackoffBucket {
    max_exponent: u32,
    factor: u64,
    rng: Mutex<Rng>,
    jitter_milis: Range<u64>,
    backoffs: Arc<Mutex<HashMap<String, BackoffBucketAttempt>>>,
}

impl Default for ExponentialBackoffBucket {
    fn default() -> Self {
        ExponentialBackoffBucket {
            max_exponent: BACKOFF_MAX_EXPONENT,
            factor: BACKOFF_FACTOR,
            rng: Mutex::new(fastrand::Rng::new()),
            jitter_milis: BACKOFF_JITTER_MS_MIN..BACKOFF_JITTER_MS_MAX,
            backoffs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ExponentialBackoffBucket {
    pub async fn wait_model_backoff(&self, model_name: &str) {
        let deadline = {
            let now = Instant::now();
            let backoffs = self.backoffs.lock().await;
            let Some(model_backoff_attempt) = backoffs.get(model_name) else {
                return;
            };
            if model_backoff_attempt.deadline < now {
                return;
            }
            debug!(
                model_name = model_name,
                attempt = model_backoff_attempt.current_attempt,
                "Waiting for model backoff"
            );
            model_backoff_attempt.deadline
        };
        // NOTE: this is here so that not all waiters will wake up exactly the same time. That
        // could easily overwhelm the API once again. Instead we opt to introduce some randomness
        let jitter = self.rng.lock().await.u64(self.jitter_milis.clone());
        sleep_until(deadline + Duration::from_millis(jitter)).await;
    }

    pub async fn increase_model_wait_time(&self, model_name: &str) {
        let mut backoffs = self.backoffs.lock().await;
        let model_backoff_attempt = match backoffs.get_mut(model_name) {
            Some(backoff_attempt) => backoff_attempt,
            None => {
                backoffs.insert(model_name.to_owned(), BackoffBucketAttempt::default());
                backoffs.get_mut(model_name).unwrap()
            }
        };
        // No other thread has set the next deadlock in the future
        if model_backoff_attempt.deadline < Instant::now() {
            let next_deadline =
                model_backoff_attempt.increase_next_deadline(self.factor, self.max_exponent);
            model_backoff_attempt.deadline = next_deadline;
        }
    }

    pub async fn clear_model_backoff(&self, model_name: &str) {
        let mut backoffs = self.backoffs.lock().await;
        if let Some(backoff_attempt) = backoffs.get_mut(model_name) {
            // If the deadline has already passed and no other request has set it to the future
            if backoff_attempt.deadline < Instant::now() {
                debug!(model_name = model_name, "Clearing model backoff state");
                backoff_attempt.clear_backoff();
            }
        }
    }
}
