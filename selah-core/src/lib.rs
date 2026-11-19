//! Core library for Selah: references, versification, import, storage and
//! search. It has no GTK dependency so it can be tested and fuzzed on its own.

pub mod books;
pub mod import;
pub mod refs;
pub mod store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("USFM line {line}: {message}")]
    Usfm { line: usize, message: String },
    #[error("translation metadata: {0}")]
    Meta(String),
    #[error("invalid translation data: {0}")]
    InvalidData(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
