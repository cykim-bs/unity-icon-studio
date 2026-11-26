//! Icon Studio core. The desktop app (src-tauri) and the command line (`uis`) call the same
//! functions, so everything can be checked and tuned without the window.

pub mod capture;
pub mod cli;
pub mod generate;
pub mod home;
pub mod hooks;
pub mod image;
pub mod journal;
pub mod packs;
pub mod prefabs;
pub mod proc;
pub mod project;
pub mod projects;
pub mod prompt;
pub mod settings;
pub mod unity;

/// Version of this core, shown by `uis --version` and the app's About tab.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
