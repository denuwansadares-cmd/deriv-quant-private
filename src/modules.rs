//! Shared module boundary for the trading bot.
//!
//! The implementation is being migrated incrementally from `main.rs`; keeping
//! this boundary in place lets each extraction compile independently.

pub mod api;
pub mod market;
pub mod models;
pub mod risk;
pub mod strategy;
