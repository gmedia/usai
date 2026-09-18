//! The HTTP workload host (`GOAL.md` §10–§15; contracts C6, C7, C11).
//!
//! ```text
//! network → route → decode → boundary validation → auth* → admit → world → encode → commit
//! ```
//!
//! Everything before `admit` runs without a world; a request that cannot
//! become application work never pays for one. (*Auth resolvers are
//! application code, so in v0 they run inside the world; ADR-0004.)

pub mod pipeline;
pub mod router;
pub mod server;
pub mod socket;
pub mod stream;

pub use pipeline::{HttpConfig, HttpHost};
pub use server::{serve, serve_internal};
