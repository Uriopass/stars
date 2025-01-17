//! The path definition in SDF.
//!
//! This mod is analogous to `hier_name.rs` in spef and
//! netlistdb.

use compact_str::CompactString;
use either::Either;
use std::hash::Hash;

/// An optional bus definition.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum SDFBus {
    None,
    SingleBit(isize),
    BitRange(isize, isize)
}

#[derive(Debug)]
/// One instance/pin path in SDF.
pub struct SDFPath {
    pub path: Vec<CompactString>,
    pub bus: SDFBus
}

/// A view of hierarchy that works with netlistdb's
/// GeneralHierName polymorphism, except that it has a
/// non-static reference that prevents it from being
/// GeneralHierName.
#[derive(Debug, Copy, Clone)]
pub struct SDFPathHierView<'i>(&'i [CompactString]);

/// A view of hierarchy that works with netlistdb's
/// GeneralHierName polymorphism.
/// This struct is unsafe because we transmute to it
/// to 'static.
#[derive(Debug, Copy, Clone)]
pub struct SDFPathHierViewStatic(&'static [CompactString]);

impl SDFPath {
    #[inline]
    pub fn to_cell_hier<'i>(&'i self) -> SDFPathHierView<'i> {
        assert_eq!(self.bus, SDFBus::None);
        SDFPathHierView(&self.path[..])
    }

    #[inline]
    pub fn to_pin_hiers<'i>(&'i self) -> impl Iterator<Item = (
        SDFPathHierView<'i>, &'i CompactString, Option<isize>
    )> {
        let hier = SDFPathHierView(&self.path[..self.path.len() - 1]);
        let pin = &self.path[self.path.len() - 1];
        use Either::*;
        match self.bus {
            SDFBus::None => Left(Some((hier, pin, None)).into_iter()),
            SDFBus::SingleBit(i) => Left(Some((hier, pin, Some(i))).into_iter()),
            SDFBus::BitRange(mut l, mut r) => {
                if l > r {
                    (l, r) = (r, l);
                }
                Right((l..=r).map(move |i| (hier, pin, Some(i))))
            }
        }
    }
}

impl<'i, 'j> IntoIterator for &'i SDFPathHierView<'j> {
    type Item = &'j CompactString;
    type IntoIter = std::iter::Rev<std::slice::Iter<'j, CompactString>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().rev()
    }
}

impl<'i> IntoIterator for &'i SDFPathHierViewStatic {
    type Item = &'i CompactString;
    type IntoIter = std::iter::Rev<std::slice::Iter<'i, CompactString>>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().rev()
    }
}

impl<'i> Hash for SDFPathHierView<'i> {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // reversed order, correspond to netlistdb.
        for s in self.0.iter().rev() {
            s.hash(state);
        }
    }
}

impl Hash for SDFPathHierViewStatic {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // reversed order, correspond to netlistdb.
        for s in self.0.iter().rev() {
            s.hash(state);
        }
    }
}

impl<'i> SDFPathHierView<'i> {
    #[inline]
    pub unsafe fn erase_lifetime(self) -> SDFPathHierViewStatic {
        SDFPathHierViewStatic(std::slice::from_raw_parts(
            self.0.as_ptr(), self.0.len()
        ))
    }
}
