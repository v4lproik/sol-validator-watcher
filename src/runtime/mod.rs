//! Runtime lifecycle and external process boundaries.
//!
//! Application wiring, HTTP serving, polling, and transactional reload state are
//! split by ownership and composed through `run`.

mod app;
mod http;
mod poller;
mod state;

pub(crate) use app::run;
pub(crate) use http::check_health;

#[cfg(test)]
mod test_support;
