//! Whether this computer's connection to the internet is metered (a
//! phone's hotspot, a mobile data plan), for a backup by itself to cloud
//! storage to wait rather than spend someone's data allowance.
//!
//! | | Windows | macOS | Linux |
//! | --- | --- | --- | --- |
//! | Metered | the Network List Manager's connection cost | not known | not known |
//!
//! Where it is not known the answer is `None`, and nothing waits for it:
//! the settings say plainly that this computer cannot tell.

/// The connection cost flags Windows gives (`NLM_CONNECTION_COST`), as
/// numbers, so what they mean is tested on every computer.
pub mod cost {
    pub const UNRESTRICTED: u32 = 0x1;
    pub const FIXED: u32 = 0x2;
    pub const VARIABLE: u32 = 0x4;
    pub const OVER_DATA_LIMIT: u32 = 0x1_0000;
    pub const CONGESTED: u32 = 0x2_0000;
    pub const ROAMING: u32 = 0x4_0000;
    pub const APPROACHING_DATA_LIMIT: u32 = 0x8_0000;
}

/// True when this computer can say whether its connection is metered.
pub const CAN_TELL: bool = cfg!(windows);

/// True on a metered connection, false on one that is not, `None` when
/// this computer cannot say or has no connection.
pub fn metered() -> Option<bool> {
    imp::cost().map(is_metered)
}

/// Whether a connection with these cost flags is metered: one paid for by
/// how much is sent (variable), with a fixed allowance, over its limit, or
/// roaming. Unrestricted alone is not; neither is a connection only busy.
pub fn is_metered(flags: u32) -> bool {
    flags & (cost::FIXED | cost::VARIABLE | cost::OVER_DATA_LIMIT | cost::ROAMING | cost::APPROACHING_DATA_LIMIT) != 0
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Networking::NetworkListManager::{INetworkCostManager, NetworkListManager};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    /// The cost of the connection the computer uses for the internet.
    pub fn cost() -> Option<u32> {
        // SAFETY: COM is started for this thread and ended only if this
        // started it; the manager is a COM object Windows makes and frees
        // itself; GetCost writes one number into a live local, and no
        // destination address is given, which asks about the machine's
        // connection as a whole.
        unsafe {
            let started = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let asked = (|| {
                let manager: INetworkCostManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
                let mut cost = 0u32;
                manager.GetCost(&mut cost, std::ptr::null()).ok()?;
                Some(cost)
            })();
            if started {
                CoUninitialize();
            }
            asked
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn cost() -> Option<u32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_paid_for_by_use_or_with_an_allowance_or_roaming_is_metered() {
        assert!(!is_metered(cost::UNRESTRICTED));
        assert!(!is_metered(cost::UNRESTRICTED | cost::CONGESTED), "busy is not metered");
        assert!(!is_metered(0));
        for metered in [
            cost::FIXED,
            cost::VARIABLE,
            cost::OVER_DATA_LIMIT,
            cost::ROAMING,
            cost::APPROACHING_DATA_LIMIT,
        ] {
            assert!(is_metered(metered), "{metered:#x}");
        }
    }

    #[test]
    fn asking_never_fails_on_any_computer() {
        let said = metered();
        if !CAN_TELL {
            assert_eq!(said, None, "says honestly it cannot tell");
        }
    }
}
