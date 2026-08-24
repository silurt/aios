use aios_types::{ApiError, ErrorKind};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no project matching {0:?}")]
    ProjectNotFound(String),

    #[error("{path} is already registered as {slug:?}")]
    PathAlreadyRegistered { path: String, slug: String },

    #[error("slug {0:?} is already taken")]
    SlugTaken(String),

    #[error("{0}")]
    Invalid(String),

    #[error("{0} is not a directory")]
    NotADirectory(String),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("config at {path}: {source}")]
    Config {
        path: String,
        #[source]
        source: toml::de::Error,
    },

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::ProjectNotFound(_) => ErrorKind::NotFound,
            Error::PathAlreadyRegistered { .. } | Error::SlugTaken(_) => ErrorKind::AlreadyExists,
            Error::Invalid(_) | Error::NotADirectory(_) => ErrorKind::InvalidArgument,
            _ => ErrorKind::Internal,
        }
    }
}

/// Every failure crosses the boundary as a typed [`ApiError`] so clients branch
/// on `kind` rather than parsing prose (§15).
impl From<&Error> for ApiError {
    fn from(e: &Error) -> Self {
        ApiError {
            kind: e.kind(),
            message: e.to_string(),
        }
    }
}
