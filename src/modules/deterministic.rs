use std::sync::atomic::{AtomicI64, Ordering as AtomicOrdering};

static VIRTUAL_TIME_MS: AtomicI64 = AtomicI64::new(-1);

fn activate_virtual_time(now_ms: i64) {
    VIRTUAL_TIME_MS.store(now_ms, AtomicOrdering::SeqCst);
}

#[derive(Debug, Clone)]
struct VirtualClock {
    now_ms: i64,
}

impl VirtualClock {
    fn new(now_ms: i64) -> Self { Self { now_ms } }
    fn advance_to(&mut self, now_ms: i64) {
        self.now_ms = self.now_ms.max(now_ms);
    }
}

#[derive(Debug, Clone)]
struct SeededRng {
    state: u64,
}

impl SeededRng {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }
    fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }
}

fn journal_timestamp_ms() -> i64 {
    let virtual_time = VIRTUAL_TIME_MS.load(AtomicOrdering::SeqCst);
    if virtual_time >= 0 {
        virtual_time
    } else {
        Utc::now().timestamp_millis()
    }
}

fn run_deterministic_simulation(
    symbols: &[String],
    history_path: &str,
    seed: u64,
) -> Result<()> {
    run_backtester(symbols, history_path, seed)
}

#[cfg(test)]
mod deterministic_tests {
    use super::*;

    #[test]
    fn identical_seeds_produce_identical_sequences() {
        let mut first = SeededRng::new(42);
        let mut second = SeededRng::new(42);
        let first_values = (0..32).map(|_| first.next_u64()).collect::<Vec<_>>();
        let second_values = (0..32).map(|_| second.next_u64()).collect::<Vec<_>>();
        assert_eq!(first_values, second_values);
    }

    #[test]
    fn virtual_clock_never_moves_backwards() {
        let mut clock = VirtualClock::new(1_000);
        clock.advance_to(2_000);
        clock.advance_to(1_500);
        assert_eq!(clock.now_ms, 2_000);
        VIRTUAL_TIME_MS.store(-1, AtomicOrdering::SeqCst);
    }
}
