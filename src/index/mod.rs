pub mod base;
pub mod hash;
pub mod sorted;
pub mod trie;
pub mod inverted;
pub mod ngram;

pub use base::Index;
pub use hash::HashIndex;
pub use sorted::SortedIndex;
pub use trie::TrieIndex;
pub use inverted::InvertedIndex;
pub use ngram::NgramIndex;
