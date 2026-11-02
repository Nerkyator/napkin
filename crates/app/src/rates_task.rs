//! Refreshes exchange rates in the background.
//!
//! The cache is read at startup by the window; this thread only downloads
//! what is due and hands the new rates to the main thread.

use napkin_engine::Rates;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

const CHECK_EVERY: Duration = Duration::from_secs(600);

pub fn spawn(enabled: Arc<AtomicBool>, wake: Receiver<()>, updates: async_channel::Sender<Rates>) {
    std::thread::spawn(move || {
        loop {
            if enabled.load(Ordering::Relaxed) {
                let mut cache = napkin_rates::load_cache().unwrap_or_default();
                let now = chrono::Utc::now().timestamp();
                match napkin_rates::refresh(&mut cache, now) {
                    Ok(true) => {
                        if let Err(e) = napkin_rates::save_cache(&cache) {
                            eprintln!("napkin: cannot save exchange rates: {e}");
                        }
                        if updates.send_blocking(cache.rates).is_err() {
                            return;
                        }
                    }
                    Ok(false) => {}
                    Err(e) => eprintln!("napkin: exchange rates not updated: {e}"),
                }
            }
            match wake.recv_timeout(CHECK_EVERY) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}
