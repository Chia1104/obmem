//! How obmem plugs into agent harnesses: the hook entry points each agent calls
//! (session start, stop, pre-compact) and `setup`, which writes each agent's config.

pub mod hooks;
pub mod setup;
