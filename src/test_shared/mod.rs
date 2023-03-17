#[cfg(not(feature = "std"))]
mod no_std;
#[cfg(feature = "std")]
mod with_std;

#[cfg(not(feature = "std"))]
pub use no_std::*;

#[cfg(feature = "std")]
pub use with_std::*;
