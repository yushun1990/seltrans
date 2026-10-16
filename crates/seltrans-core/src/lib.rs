//! GUI-free core for seltrans: selection capture, classification, domain
//! vocabulary, provider access and the resident IPC transport. Product and
//! architecture contracts live in docs/.

pub mod analysis;
pub mod classifier;
pub mod config;
pub mod domain;
pub mod ipc;
pub mod openai;
pub mod selection;
