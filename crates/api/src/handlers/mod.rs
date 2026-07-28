//! HTTP handler modules.
//!
//! Each sub-module corresponds to a feature area. Handlers are kept thin:
//! they validate input, call into the application layer, and serialise the
//! result. No business logic lives here.

pub mod health;
