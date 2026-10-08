//! js-yaml compatible YAML loading and dumping.

mod dump;
mod load;
mod snippet;

pub use dump::dump;
pub use load::{YamlError, load};
