//! Reference-compatibility switch.
//!
//! Phase 1 reproduced daff 1.4.2 exactly, bugs included. Phase 2 fixes those
//! bugs (see `docs/BUGS.md`) and the fixes are on by default. Turning
//! reference compatibility on restores the original behaviour of every fixed
//! bug, which the differential tests against the reference rely on.
//!
//! This is a process-wide setting meant for the command line tool and for
//! tests; library users normally leave it off.
use std::sync::atomic::{AtomicBool, Ordering};

static REFERENCE_COMPAT: AtomicBool = AtomicBool::new(false);

pub fn set_reference_compat(on: bool) {
    REFERENCE_COMPAT.store(on, Ordering::Relaxed);
}

/// True when bugs of the reference implementation must be reproduced.
pub fn reference_compat() -> bool {
    REFERENCE_COMPAT.load(Ordering::Relaxed)
}
