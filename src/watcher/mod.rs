pub mod front_matter;
pub mod source_watcher;
pub mod task_watcher;

use sha2::{Digest, Sha256};

pub(crate) fn sha256_hex(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}
