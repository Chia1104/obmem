//! Headless flow tests for the window. One binary, so Native Shell is linked once; each test
//! still runs in a process of its own (see `common::sandbox`).

mod common;

mod agent;
mod app;
mod appearance;
mod review;
mod settings;
mod sidebar;
