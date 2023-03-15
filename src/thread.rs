#[cfg(not(loom))]
pub(crate) use std::thread::*;

#[cfg(loom)]
pub(crate) use loom::thread::*;

#[cfg(loom)]
pub(crate) use std::thread::sleep;
