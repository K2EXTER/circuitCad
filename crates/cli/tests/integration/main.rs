//! The integration tests of `ccad`, in one test binary (one link, and the files run side by side instead of one binary after another):
//! `script` (scripts and their JSON answers), `mcp` (the MCP server), and the bundled models measured against their data sheets
//! (`*_models`, sharing `checks`). They run the commands of `ccad` inside the test (`common`); `mcp` and one test of `script` start the binary.

mod checks;
mod common;
mod diode_models;
mod mcp;
mod opamp_models;
mod regulator_models;
mod script;
mod transistor_models;
