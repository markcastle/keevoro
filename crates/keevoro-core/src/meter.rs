//! Counting what Keevoro has sent.
//!
//! "Is it slow, or is there just a lot to send?" has an answer only if the
//! bytes are counted as they arrive. Every upload adds to these running
//! totals when a file, or a part of a large file, has safely landed. The
//! app reads them once a second to show a speed and a time left.
//!
//! Two totals are kept: everything sent anywhere, and the part of it that
//! went over the network (to cloud storage, as against a folder on a disk).

use std::sync::atomic::{AtomicU64, Ordering};

static SENT: AtomicU64 = AtomicU64::new(0);
static OVER_NETWORK: AtomicU64 = AtomicU64::new(0);

/// Records that `bytes` more have safely arrived at a destination.
pub fn sent(bytes: u64, over_network: bool) {
    SENT.fetch_add(bytes, Ordering::Relaxed);
    if over_network {
        OVER_NETWORK.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// Bytes sent since the program started: (anywhere, over the network).
pub fn totals() -> (u64, u64) {
    (SENT.load(Ordering::Relaxed), OVER_NETWORK.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_sent_is_counted_and_the_network_share_kept_apart() {
        let (all, net) = totals();
        sent(1000, false);
        sent(250, true);
        let (all_after, net_after) = totals();
        assert_eq!(all_after - all, 1250);
        assert_eq!(net_after - net, 250);
    }
}
