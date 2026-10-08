use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Option<Instant>> = OnceLock::new();

/// Start the clock if `PEREPLYOT_PERF` is set; otherwise every call below is a cheap no-op.
pub fn init() {
    START.get_or_init(|| std::env::var_os("PEREPLYOT_PERF").map(|_| Instant::now()));
}

fn started() -> Option<Instant> {
    START.get().copied().flatten()
}

pub fn mark(label: &str) {
    if let Some(t) = started() {
        eprintln!("PERF {:>9.1} {label}", t.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Prints how long it lived, and when it ended, when dropped.
pub struct Span(Option<(Instant, String)>);

pub fn span(label: impl FnOnce() -> String) -> Span {
    Span(started().map(|_| (Instant::now(), label())))
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((began, label)) = self.0.take() {
            let took = began.elapsed().as_secs_f64() * 1000.0;
            if let Some(t) = started() {
                eprintln!(
                    "PERF {:>9.1} {label} took {took:.1} ms",
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
    }
}
