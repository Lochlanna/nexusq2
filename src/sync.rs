#[cfg(not(loom))]
pub(crate) use std::sync::Arc;

#[cfg(loom)]
pub(crate) use loom::sync::Arc;

pub mod atomic {
    #[cfg(not(loom))]
    pub(crate) use std::sync::atomic::*;

    #[cfg(loom)]
    pub(crate) use loom::sync::atomic::*;
}
