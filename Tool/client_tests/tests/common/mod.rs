//! Helpers shared between the integration test binaries.
//!
//! A subdirectory is not compiled as its own test binary, so anything in here
//! is available to every test file that declares `mod common;` without becoming
//! a test target itself.

// Not every test binary uses every helper, so an unused one here is expected
// rather than a defect.
#[allow(dead_code)]
pub mod png;
