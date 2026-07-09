// Port of internal/store/errors.go, widened into an enum: Go used three
// sentinel errors plus ad-hoc errors.New/fmt.Errorf strings; Rust carries
// the sentinels as variants and the ad-hoc messages as Other.
use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Go: ErrNotFound.
    NotFound,
    /// Go: ErrRevisionMismatch (scratchpad expected-revision guard failed).
    RevisionMismatch,
    /// Go: ErrConcurrentEdit (todo Updated changed since the caller read it).
    ConcurrentEdit,
    Io(std::io::Error),
    Json(serde_json::Error),
    /// Ad-hoc messages the Go code built inline ("lock owned by X",
    /// "line range out of bounds", "unknown edit target ...").
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => write!(f, "not found"),
            Error::RevisionMismatch => write!(f, "revision mismatch"),
            Error::ConcurrentEdit => write!(f, "todo changed since read"),
            Error::Io(e) => e.fmt(f),
            Error::Json(e) => e.fmt(f),
            Error::Other(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
