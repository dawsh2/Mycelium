//! Supervision strategies for actor error handling

/// Supervision strategy for handling actor failures
///
/// Determines what happens when an actor encounters an error or panic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SupervisionStrategy {
    /// Stop the actor permanently on error
    Stop,

    /// Restart the actor on error
    ///
    /// The actor will be restarted with a fresh state (calling its constructor again).
    Restart {
        /// Maximum number of restart attempts before giving up
        max_retries: usize,
    },

    /// Resume processing (ignore the error and continue)
    Resume,

    /// Escalate the error to a parent supervisor
    Escalate,
}

impl Default for SupervisionStrategy {
    fn default() -> Self {
        Self::Resume
    }
}

/// Restart strategy configuration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RestartStrategy {
    /// Never restart
    Never,

    /// Always restart immediately
    Immediate,

    /// Restart with exponential backoff
    ExponentialBackoff {
        /// Initial delay before first restart
        initial_delay_ms: u64,
        /// Maximum delay between restarts
        max_delay_ms: u64,
        /// Backoff multiplier (e.g., 2.0 for doubling)
        multiplier: f64,
    },

    /// Restart with fixed delay
    FixedDelay {
        /// Delay between restart attempts
        delay_ms: u64,
    },
}

impl Default for RestartStrategy {
    fn default() -> Self {
        Self::Immediate
    }
}

impl RestartStrategy {
    /// Calculate the delay for a given restart attempt
    pub fn delay_for_attempt(&self, attempt: usize) -> std::time::Duration {
        match self {
            Self::Never => std::time::Duration::from_secs(0),
            Self::Immediate => std::time::Duration::from_millis(0),
            Self::FixedDelay { delay_ms } => std::time::Duration::from_millis(*delay_ms),
            Self::ExponentialBackoff {
                initial_delay_ms,
                max_delay_ms,
                multiplier,
            } => {
                let delay = (*initial_delay_ms as f64) * multiplier.powi(attempt as i32);
                let delay = delay.min(*max_delay_ms as f64) as u64;
                std::time::Duration::from_millis(delay)
            }
        }
    }
}

/// Supervision directive issued by a supervisor
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Future use: Will be used when supervisor decision logic is implemented
pub enum Directive {
    /// Resume the actor (continue processing)
    Resume,

    /// Restart the actor with fresh state
    Restart,

    /// Stop the actor permanently
    Stop,

    /// Escalate to parent supervisor
    Escalate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_immediate_restart() {
        let strategy = RestartStrategy::Immediate;
        assert_eq!(strategy.delay_for_attempt(0).as_millis(), 0);
        assert_eq!(strategy.delay_for_attempt(10).as_millis(), 0);
    }

    #[test]
    fn test_fixed_delay() {
        let strategy = RestartStrategy::FixedDelay { delay_ms: 100 };
        assert_eq!(strategy.delay_for_attempt(0).as_millis(), 100);
        assert_eq!(strategy.delay_for_attempt(5).as_millis(), 100);
    }

    #[test]
    fn test_exponential_backoff() {
        let strategy = RestartStrategy::ExponentialBackoff {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            multiplier: 2.0,
        };

        assert_eq!(strategy.delay_for_attempt(0).as_millis(), 100);
        assert_eq!(strategy.delay_for_attempt(1).as_millis(), 200);
        assert_eq!(strategy.delay_for_attempt(2).as_millis(), 400);
        assert_eq!(strategy.delay_for_attempt(3).as_millis(), 800);

        // Should cap at max_delay_ms
        assert_eq!(strategy.delay_for_attempt(10).as_millis(), 10000);
    }

    #[test]
    fn test_supervision_strategy_default() {
        let strategy = SupervisionStrategy::default();
        assert_eq!(strategy, SupervisionStrategy::Resume);
    }
}
