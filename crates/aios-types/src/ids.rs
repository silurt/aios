//! Newtype identifiers.
//!
//! These exist so that a generated Swift or TypeScript client cannot silently
//! transpose two string arguments. They are ULIDs: lexicographically sortable
//! by creation time, which means `ORDER BY id` is `ORDER BY created_at` without
//! an index on the timestamp.

use serde::{Deserialize, Serialize};
use std::fmt;
use utoipa::ToSchema;

macro_rules! newtype_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

newtype_id!(
    /// Identifies a registered project.
    ProjectId
);
