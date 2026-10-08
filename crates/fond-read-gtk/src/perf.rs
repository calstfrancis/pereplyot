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

thread_local! {
    static WATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Report every stretch where the GTK main loop went more than 20 ms without running a timer
/// that is due every 8 ms — a stall the user would feel as the window freezing. Call from the
/// GTK thread; does nothing unless tracing is on, and only starts once.
pub fn watch_main_loop() {
    if started().is_none() || WATCHING.with(|w| w.replace(true)) {
        return;
    }
    let last = std::cell::Cell::new(Instant::now());
    glib::timeout_add_local(std::time::Duration::from_millis(8), move || {
        let now = Instant::now();
        let late = now.duration_since(last.replace(now)).as_secs_f64() * 1000.0 - 8.0;
        if late > 20.0 {
            mark(&format!("stall {late:.0} ms"));
        }
        glib::ControlFlow::Continue
    });
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
