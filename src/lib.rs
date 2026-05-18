use std::sync::atomic::AtomicBool;
pub static RUNNING: AtomicBool = AtomicBool::new(true);

pub mod analysis;
pub mod capture;
pub mod constants;
pub mod pipeline;
pub mod replay;
pub mod replay_discovery;
pub mod report;
pub mod sync;
pub mod types;
pub mod version;
