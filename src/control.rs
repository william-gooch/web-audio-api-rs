//! Scheduler and Controller for precise timings

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::AtomicF64;

/// Helper struct to start and stop audio streams
#[derive(Clone, Debug)]
pub(crate) struct Scheduler {
    start: Arc<AtomicF64>,
    stop: Arc<AtomicF64>,
}

impl Scheduler {
    /// Create a new Scheduler. Initial playback state will be: inactive.
    pub fn new() -> Self {
        Self {
            start: Arc::new(AtomicF64::new(f64::MAX)),
            stop: Arc::new(AtomicF64::new(f64::MAX)),
        }
    }

    /// Retrieve playback start value
    pub fn get_start_at(&self) -> f64 {
        self.start.load()
    }

    /// Schedule playback start at this timestamp
    pub fn start_at(&self, start: f64) {
        // todo panic on invalid values, or when already called
        self.start.store(start);
    }

    /// Retrieve playback stop value
    pub fn get_stop_at(&self) -> f64 {
        self.stop.load()
    }

    /// Stop playback at this timestamp
    pub fn stop_at(&self, stop: f64) {
        // todo panic on invalid values, or when already called
        self.stop.store(stop);
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper struct to control audio streams
#[derive(Clone, Debug)]
pub(crate) struct Controller {
    scheduler: Arc<Scheduler>,
    loop_: Arc<AtomicBool>,
    loop_start: Arc<AtomicF64>,
    loop_end: Arc<AtomicF64>,
    offset: Arc<AtomicF64>,
    duration: Arc<AtomicF64>,
}

impl Controller {
    /// Create a new Controller. It will not be active
    pub fn new() -> Self {
        Self {
            scheduler: Arc::new(Scheduler::new()),
            loop_: Arc::new(AtomicBool::new(false)),
            loop_start: Arc::new(AtomicF64::new(0.)),
            loop_end: Arc::new(AtomicF64::new(f64::MAX)),
            offset: Arc::new(AtomicF64::new(f64::MAX)),
            duration: Arc::new(AtomicF64::new(f64::MAX)),
        }
    }

    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    pub fn loop_(&self) -> bool {
        self.loop_.load(Ordering::SeqCst)
    }

    pub fn set_loop(&self, loop_: bool) {
        self.loop_.store(loop_, Ordering::SeqCst);
    }

    pub fn loop_start(&self) -> f64 {
        self.loop_start.load()
    }

    pub fn set_loop_start(&self, loop_start: f64) {
        self.loop_start.store(loop_start);
    }

    pub fn loop_end(&self) -> f64 {
        self.loop_end.load()
    }

    pub fn set_loop_end(&self, loop_end: f64) {
        self.loop_end.store(loop_end);
    }

    pub fn offset(&self) -> f64 {
        self.offset.load()
    }

    pub fn set_offset(&self, offset: f64) {
        self.offset.store(offset);
    }

    pub fn duration(&self) -> f64 {
        self.duration.load()
    }

    pub fn set_duration(&self, duration: f64) {
        self.duration.store(duration)
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller() {
        let controller = Controller::new();

        assert!(!controller.loop_());
        assert!(controller.loop_start() == 0.);
        assert!(controller.loop_end() == f64::MAX);
    }
}
