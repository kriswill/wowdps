//! A token group as data a config can name: one macro writes the struct
//! and the table of its field names together, so a field added to a group
//! is at once a key `[themes.<name>.<group>]` accepts, and a key the config
//! misspells is reported by name rather than silently dropped.

/// `tokens! { /// docs  pub struct Name: Type { /// docs  field, … } }` —
/// a `Copy` struct whose fields are all `Type`, with `NAMES` (every field
/// as a config spells it), `get` and `get_mut` by name.
macro_rules! tokens {
    (
        $(#[$meta:meta])*
        pub struct $name:ident: $ty:ty {
            $( $(#[$fmeta:meta])* $field:ident, )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
        }

        impl $name {
            /// Every field, as a config names it, in declaration order.
            pub const NAMES: &'static [&'static str] = &[$(stringify!($field)),*];

            /// The field a config names.
            pub fn get(&self, name: &str) -> Option<$ty> {
                match name {
                    $( stringify!($field) => Some(self.$field), )*
                    _ => None,
                }
            }

            /// The field a config names, to set.
            pub fn get_mut(&mut self, name: &str) -> Option<&mut $ty> {
                match name {
                    $( stringify!($field) => Some(&mut self.$field), )*
                    _ => None,
                }
            }
        }
    };
}

pub(crate) use tokens;
