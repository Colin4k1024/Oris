//! EvoKernel orchestration: mutation capture, validation, capsule construction, and replay-first reuse.

pub mod adapters;
pub mod confidence_daemon;
mod core;
pub mod signal_extractor;

/// Experimental external agent contract facade re-exported through EvoKernel.
pub mod agent_contract {
    pub use oris_agent_contract::*;
}

/// Experimental local EVU and reputation accounting facade re-exported through EvoKernel.
pub mod economics {
    pub use oris_economics::*;
}

/// Experimental Oris Evolution Network protocol facade re-exported through EvoKernel.
pub mod evolution_network {
    pub use oris_evolution_network::*;
}

/// Experimental governor policy facade re-exported through EvoKernel.
pub mod governor {
    pub use oris_governor::*;
}

/// Experimental OUSL spec compiler facade re-exported through EvoKernel.
pub mod spec_contract {
    pub use oris_spec::*;
}

pub use core::*;
