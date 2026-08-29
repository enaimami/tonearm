//! `tune-core` — sağlayıcıdan bağımsız dinleme kimliği çekirdeği.
//!
//! **Altın Kural:** bütün mantık buradadır. CLI, GUI ve mobil bağlamalar
//! yalnızca bu API'yi çağırır; hiçbiri kendi başına iş yapmaz.

// CLAUDE.md: çekirdekte `unwrap`/`expect`/`panic!` yok — **testler hariç**.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod config;
pub mod diag;
pub mod error;
pub mod identity;
pub mod ids;
pub mod import;
pub mod library;
pub mod model;
pub mod playback;
pub mod provider;
pub mod session;
pub mod stats;
pub mod wrapped;

pub use error::{Error, ErrorKind, Result};
