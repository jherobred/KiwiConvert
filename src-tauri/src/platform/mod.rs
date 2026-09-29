//! Windows integration: the drag gesture, the wheel's drop target, overlay windows, shell
//! thumbnails, and revealing files in File Explorer.

pub mod activation;
pub mod droptarget;
pub mod gesture;
pub mod overlay;
pub mod shell;

pub use shell::{reveal, thumbnail_png};
