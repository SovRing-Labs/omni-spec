//! omni-spec — our own code built from PUBLISHED STANDARDS only (the module list is in README.md).
//! Rules: implement from the specification text; cite the spec section beside the code it governs; no third-party code read or
//! copied; every module ships its official conformance data as a test. Modules are added by the SPEC0 wave (one module per task).

pub mod brotli;
pub mod gif;
pub mod html;
mod brotli_tables;
pub mod inflate;
pub mod jpeg;
pub mod json;
pub mod ooxml;
pub mod opus;
pub mod opus_celt;
pub mod opus_silk;
mod opus_silk_tables;
pub mod otshape;
pub mod png;
pub mod sfnt;
pub mod sha256;
pub mod uax14;
mod uax14_tables;
pub mod uax29;
pub mod uax9;
pub mod webm;
pub mod webp;
pub mod woff2;
pub mod xml;
pub mod deflate;
pub mod zip;
mod uax29_tables;
mod uax9_tables;
mod html_entities;
