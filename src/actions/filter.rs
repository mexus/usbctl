//! Filtering utilities.

use crate::device::Device;

/// Device filter trait.
pub trait Filter {
    /// Checks if the given device should be yielded.
    fn filter(&mut self, device: &Device) -> bool;
}

/// A no-op filter which yields all the supplied devices.
#[derive(Debug, Default)]
pub struct NoOpFilter(());

impl Filter for NoOpFilter {
    #[inline]
    fn filter(&mut self, _device: &Device) -> bool {
        true
    }
}

/// A filter that wraps a closure.
pub struct ClosureFilter<F>(F);

impl<F> ClosureFilter<F> {
    /// Creates a [Filter] from the provided closure.
    pub fn new(closure: F) -> Self
    where
        F: FnMut(&Device) -> bool,
    {
        Self(closure)
    }
}

impl<F> Filter for ClosureFilter<F>
where
    F: FnMut(&Device) -> bool,
{
    #[inline]
    fn filter(&mut self, device: &Device) -> bool {
        (self.0)(device)
    }
}

/// [Filter] extension trait.
pub trait FilterExt: Filter {
    /// Calls the `next` filter when the current one succeeds.
    fn chain<NextFilter>(self, next: NextFilter) -> ChainFilter<Self, NextFilter>
    where
        Self: Sized,
        NextFilter: Filter,
    {
        ChainFilter {
            first: self,
            second: next,
        }
    }
}

/// A filter that combines two filters.
pub struct ChainFilter<First, Second> {
    first: First,
    second: Second,
}

impl<First, Second> Filter for ChainFilter<First, Second>
where
    First: Filter,
    Second: Filter,
{
    fn filter(&mut self, device: &Device) -> bool {
        self.first.filter(device) && self.second.filter(device)
    }
}
