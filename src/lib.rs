pub mod cli;
pub mod dict;
pub mod download;
pub mod lang;
pub mod models;
pub mod path;
pub mod tags;
pub mod utils;

pub use dict::make_dict;

use fxhash::FxBuildHasher;
use indexmap::{IndexMap, IndexSet};

pub type Map<K, V> = IndexMap<K, V, FxBuildHasher>;
pub type Set<K> = IndexSet<K, FxBuildHasher>;
