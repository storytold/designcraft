/// Milliseconds from a monotonic process-local origin, on native and web alike.
///
/// Screenshot settlement, deadlines and retries must not use wall time: an NTP/manual clock jump
/// would expire or postpone a capture, and `Context::input(i.time)` is stale inside `run_logic`.
/// `web_time::Instant` is `std::time::Instant` natively and `performance.now()` on wasm.
pub fn now_ms() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<web_time::Instant> = OnceLock::new();
    START.get_or_init(web_time::Instant::now).elapsed().as_secs_f64() * 1000.0
}
