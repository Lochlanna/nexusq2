#[cfg(not(loom))]
pub(crate) use core::hint::*;

#[cfg(loom)]
pub(crate) use loom::hint::*;
