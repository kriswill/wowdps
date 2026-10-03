//! A theme's words — its name, its labels, a face's family — as either a
//! literal compiled in (a built-in's) or a string shared from a config (a
//! theme of the user's own). A theme built from a config is then an owned
//! value like any other, freed with the last thing holding it, rather than
//! leaked for the life of the process so it could pass for a built-in.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// A theme's words: cheap to clone either way.
#[derive(Clone)]
pub enum Text {
    /// Compiled in.
    Static(&'static str),
    /// From a config, shared by every clone.
    Shared(Arc<str>),
}

impl Text {
    pub fn as_str(&self) -> &str {
        match self {
            Text::Static(s) => s,
            Text::Shared(s) => s,
        }
    }
}

impl Deref for Text {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Text::Shared(Arc::from(s))
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Text {}

impl PartialEq<str> for Text {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Text {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_kind_reads_and_compares_as_its_words() {
        let a = Text::Static("onyx");
        let b = Text::from("onyx");
        assert_eq!(a, b);
        assert_eq!(a, "onyx");
        assert_eq!(&*b, "onyx");
        assert_eq!(format!("{b} {a:?}"), "onyx \"onyx\"");
    }
}
