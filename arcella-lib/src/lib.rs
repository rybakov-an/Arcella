// arcella-lib/src/lib.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! ALME (Arcella Local Management Extensions) protocol definitions.
//!
//! This crate defines the shared request/response structures used by both
//! the Arcella daemon (server) and clients (e.g., CLI, GUI, tests).

pub mod alme;
pub mod error;
pub mod spec;