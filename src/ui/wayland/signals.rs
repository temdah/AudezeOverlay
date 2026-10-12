//! Coalesce worker notifications while a menu is open or the UI is busy.
//! Readings already live in the shared snapshot; queuing every wake is wasteful.
use audeze_overlay::device::command::Signal;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Default)]
pub(super) struct Pending(AtomicU8);

impl Pending {
    /// True only when the event loop needs waking.
    pub fn push(&self, signal: Signal) -> bool {
        let bit = match signal {
            Signal::Status => 1,
            Signal::Control => 2,
        };
        self.0.fetch_or(bit, Ordering::AcqRel) == 0
    }

    pub fn take(&self) -> (bool, bool) {
        let bits = self.0.swap(0, Ordering::AcqRel);
        (bits & 1 != 0, bits & 2 != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_ui_keeps_both_notifications_without_a_backlog() {
        let pending = Pending::default();
        assert!(pending.push(Signal::Status));
        for _ in 0..10_000 {
            assert!(!pending.push(Signal::Control));
            assert!(!pending.push(Signal::Status));
        }
        assert_eq!(pending.take(), (true, true));
        assert_eq!(pending.take(), (false, false));
        assert!(pending.push(Signal::Control));
        assert_eq!(pending.take(), (false, true));
    }
}
