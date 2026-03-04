use std::future::Future;
use std::time::Duration;

use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::error::TranscriptionError;
use crate::request_policy::RequestPolicy;

pub async fn retry_with_cancellation<T, Op, Fut, Notify>(
    request_policy: RequestPolicy,
    cancellation: &CancellationContext,
    mut operation: Op,
    mut notify: Notify,
) -> CancellationResult<T, TranscriptionError>
where
    Op: FnMut() -> Fut,
    Fut: Future<Output = CancellationResult<T, TranscriptionError>>,
    Notify: FnMut(&TranscriptionError, Duration),
{
    let mut retries_used = 0_u32;

    loop {
        cancellation.check()?;

        let result = operation().await;
        cancellation.check()?;

        match result {
            Ok(value) => return Ok(value),
            Err(CancellationError::Cancelled) => return Err(CancellationError::Cancelled),
            Err(CancellationError::Error(err)) => {
                if !err.is_retryable() || retries_used >= request_policy.max_retries {
                    if err.is_rate_limit_error() && retries_used >= request_policy.max_retries {
                        return Err(CancellationError::Error(
                            TranscriptionError::RateLimitExhausted {
                                retries: request_policy.max_retries,
                            },
                        ));
                    }

                    return Err(CancellationError::Error(err));
                }

                let delay = retry_delay(request_policy, retries_used);
                notify(&err, delay);
                retries_used += 1;
                cancellation.sleep_async(delay).await?;
            }
        }
    }
}

fn retry_delay(request_policy: RequestPolicy, retry_index: u32) -> Duration {
    let mut delay = request_policy.base_delay;

    for _ in 0..retry_index {
        delay = delay.saturating_mul(2);
        if delay >= request_policy.max_delay {
            return request_policy.max_delay;
        }
    }

    delay.min(request_policy.max_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_request_policy() -> RequestPolicy {
        RequestPolicy::new(
            Duration::from_millis(20),
            3,
            Duration::from_millis(5),
            Duration::from_millis(20),
        )
    }

    #[test]
    fn cancellation_before_first_attempt_returns_cancelled() {
        let cancellation = CancellationContext::new();
        cancellation.cancel();
        let mut attempts = 0;

        let result = crate::runtime::block_on(retry_with_cancellation(
            fast_request_policy(),
            &cancellation,
            || {
                attempts += 1;
                std::future::ready(Ok::<_, CancellationError<TranscriptionError>>("ok"))
            },
            |_, _| {},
        ));

        assert!(matches!(result, Err(CancellationError::Cancelled)));
        assert_eq!(attempts, 0);
    }

    #[test]
    fn cancellation_during_retry_sleep_stops_before_next_attempt() {
        let cancellation = CancellationContext::new();
        let cancellation_for_thread = cancellation.clone();
        let mut attempts = 0;
        let request_policy = RequestPolicy::new(
            Duration::from_millis(20),
            3,
            Duration::from_millis(100),
            Duration::from_millis(100),
        );

        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            cancellation_for_thread.cancel();
        });

        let result = crate::runtime::block_on(retry_with_cancellation(
            request_policy,
            &cancellation,
            || {
                attempts += 1;
                std::future::ready(Err::<(), _>(CancellationError::Error(
                    TranscriptionError::Network("temporary".into()),
                )))
            },
            |_, _| {},
        ));

        cancel_thread.join().unwrap();

        assert!(matches!(result, Err(CancellationError::Cancelled)));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn exhausted_rate_limit_converts_to_rate_limit_exhausted() {
        let mut attempts = 0;

        let result = crate::runtime::block_on(retry_with_cancellation(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                attempts += 1;
                std::future::ready(Err::<(), _>(CancellationError::Error(
                    TranscriptionError::Api {
                        status: 429,
                        message: "rate limited".into(),
                    },
                )))
            },
            |_, _| {},
        ));

        assert!(matches!(
            result,
            Err(CancellationError::Error(
                TranscriptionError::RateLimitExhausted { retries: 3 }
            ))
        ));
        assert_eq!(attempts, 4);
    }
}
