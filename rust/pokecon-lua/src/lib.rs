//! pokecon-lua — LuaJIT runtime integration for Poke-Controller.
//!
//! This crate provides a Lua (LuaJIT-compatible via `mlua`) runtime that can be
//! embedded into the Poke-Controller Rust core.  It exposes the `pokecon` API
//! table to Lua scripts, giving them access to:
//!
//! - **Input operations**: `press`, `hold`, `hold_end`, `move_stick`, `wait`,
//!   `direct_serial`
//! - **Logging**: `log`, `print_s`, `print_t`, `print_t1`, `print_t2`
//! - **Event system**: `on`, `off`, `emit`, `define_event`, `autocmd`
//! - **Notifications**: `discord_text`, `line_text`
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │  Lua Script                         │
//! │  (user's init.lua / modules/*.lua)  │
//! └──────────┬──────────────────────────┘
//!            │ pokecon.*
//!            ▼
//! ┌─────────────────────────────────────┐
//! │  pokecon-lua (this crate)           │
//! │  ┌───────────┐  ┌────────────────┐  │
//! │  │ api.rs    │  │ runtime.rs     │  │
//! │  │ PokeConApi│  │ LuaRuntime     │  │
//! │  │ ApiHandle │  │ (async wrapper)│  │
//! │  └─────┬─────┘  └───────┬────────┘  │
//! │        │                │            │
//! │        └── EventBus ────┘            │
//! └─────────────────────────────────────┘
//!            │
//!            ▼ pokecon-events
//! ┌─────────────────────────────────────┐
//! │  pokecon-events / pokecon-core      │
//! │  (event dispatch, serial, camera)    │
//! └─────────────────────────────────────┘
//! ```
//!
//! # Quick start
//!
//! ```rust,no_run
//! use pokecon_lua::runtime::LuaRuntime;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut rt = LuaRuntime::new()?;
//! rt.load_script("pokecon.log('Hello from Lua!')").await?;
//! rt.call_function("my_func", (42i64,)).await?;
//! # Ok(())
//! # }
//! ```

pub mod api;
pub mod runtime;

// Re-export key types for convenience.
pub use api::{ApiHandle, PokeConApi};
pub use runtime::{LuaRuntime, LuaRuntimeError};
