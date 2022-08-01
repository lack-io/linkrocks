pub mod changer;
pub mod restore;

pub use changer::{Changer, MapChange, MapChangeType};
pub use restore::restore;

use crate::tracker::Configuration;

#[inline]
pub(crate) fn joint(cfg: &Configuration) -> bool {
    !cfg.voters().outgoing.is_empty()
}
