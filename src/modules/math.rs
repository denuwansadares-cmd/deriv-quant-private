// ═══════════════════════════════════════════════════════════════════════════
// MATH HELPERS
// ═══════════════════════════════════════════════════════════════════════════

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

fn mean(data: &[f64]) -> f64 {
    if data.is_empty() {
        0.0
    } else {
        data.iter().sum::<f64>() / data.len() as f64
    }
}

fn std_dev(data: &[f64], m: f64) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    (data.iter().map(|x| (x - m).powi(2)).sum::<f64>() / data.len() as f64).sqrt()
}

fn ema(data: &[f64], period: usize) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut e = data[0];
    for &v in &data[1..] {
        e = v * alpha + e * (1.0 - alpha);
    }
    e
}

// ═══════════════════════════════════════════════════════════════════════════
// RING BUFFER
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct RingBuffer<T> {
    data: VecDeque<T>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    fn new(capacity: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(capacity.min(256)),
            capacity,
        }
    }
    fn push(&mut self, item: T) {
        while self.data.len() >= self.capacity {
            self.data.pop_front();
        }
        self.data.push_back(item);
    }
    fn len(&self) -> usize {
        self.data.len()
    }
    fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }
}

