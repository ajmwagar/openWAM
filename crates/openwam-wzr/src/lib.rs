//! WAM 2.4 `.wzr` project files: strict typed reader, byte-exact writer, and an
//! exporter from a verified openWAM CAM artifact. Format: `docs/wzr-format.md`.
//!
//! Licence boundary: this crate is MIT OR Apache-2.0 and never links openWAM's
//! AGPL `sketch-intake` crate. It consumes that crate's CLI JSON (and runs the
//! CLI as a child process for the verify gate).
//!
//! Authority: a `.wzr` is a WAM *project* file, not machine output. This crate
//! never produces G-code, never talks to a machine, and reports
//! `machine_output_enabled: false` and `hardware_qualified: false`.

pub mod bbox;
mod check;
pub mod export;
pub mod json;
pub mod model;
pub mod summary;

pub use json::{WzrError, read, write};

/// Capability flags this crate adds next to openWAM's own (which stay unchanged).
pub fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "schema": "openwam-wzr.capabilities.v1",
        "wzr_format": "WAM 2.4.0 project (JSON)",
        "wzr_file_export": true,
        "machine_output_enabled": false,
        "hardware_qualified": false,
        "operations": ["capabilities", "read", "roundtrip", "export_wzr"],
        "export_gate": "export_wzr runs `openwam request` verify_cam itself and refuses unless verified:true for the same project_sha256",
    })
}
