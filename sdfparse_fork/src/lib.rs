//! Standard delay format (SDF) parser for EDA applications.
//!
//! ## How to use
//! See [`SDF::parse_str`].
//!
//! A number of features, including timing checks, are unsupported
//! at this moment.

use compact_str::CompactString;

/// The main entry of SDF.
#[derive(Debug)]
pub struct SDF {
    pub header: SDFHeader,
    pub cells: Vec<SDFCell>
}

/// The header information of SDF.
#[derive(Debug)]
pub struct SDFHeader {
    pub sdf_version: CompactString,
    pub design_name: Option<CompactString>,
    pub date: Option<CompactString>,
    pub vendor: Option<CompactString>,
    pub program: Option<CompactString>,
    pub program_version: Option<CompactString>,
    pub hier_divider: char,
    pub voltage: Option<SDFValue>,
    pub process: Option<CompactString>,
    pub temperature: Option<SDFValue>,
    pub timescale: f32
}

mod path;
pub use path::{ SDFPath, SDFBus };

/// One port in SDF
#[derive(Debug)]
pub struct SDFPort {
    pub port_name: CompactString,
    pub bus: SDFBus
}

/// One value specification in SDF with at most 3 corners.
#[derive(Debug)]
pub enum SDFValue {
    None,
    Single(f32),
    Multi(Option<f32>, Option<f32>, Option<f32>)
}

/// One SDF cell containing delay and constraint definitions.
#[derive(Debug)]
pub struct SDFCell {
    pub celltype: CompactString,
    pub instance: Option<SDFPath>,
    pub delays: Vec<SDFDelay>,
    // timing checks not implemented (yet).
    // pub timing_checks: Vec<SDFTimingCheck>
}

/// SDF interconnect delay.
#[derive(Debug)]
pub struct SDFDelayInterconnect {
    pub a: SDFPath,
    pub b: SDFPath,
    pub delay: Vec<SDFValue>
}

/// SDF IO path delay.
#[derive(Debug)]
pub struct SDFDelayIOPath {
    pub a: SDFPortSpec,
    pub b: SDFPort,
    /// The retain value of SDF IO path delay.
    /// See SDF docs or synopsys VCS docs for information.
    pub retain: Option<Vec<SDFValue>>,
    pub delay: Vec<SDFValue>
}

/// One SDF delay definition.
#[derive(Debug)]
pub enum SDFDelay {
    Interconnect(SDFDelayInterconnect),
    IOPath(SDFIOPathCond, SDFDelayIOPath)
}

/// IO path delay condition, simple version.
#[derive(Debug)]
pub enum SDFIOPathCond {
    None,
    /// `X == 1'b0 && Y == 1'b1` ...
    Cond(Vec<(SDFPort, bool)>),
    CondElse
}

/// A port with edge specification
#[derive(Debug)]
pub struct SDFPortSpec {
    pub edge_type: SDFPortEdge,
    pub port: SDFPort
}

/// The types of specified edges.
#[derive(Debug)]
pub enum SDFPortEdge {
    None,
    Posedge, Negedge,
    T01, T10, T0Z, TZ1, T1Z, TZ0
}

mod sdfpest;

impl SDF {
    /// Parse a SDF source string to the SDF object, or an error message with line number.
    /// This is the main entry.
    #[inline]
    pub fn parse_str(s: &str) -> Result<SDF, String> {
        sdfpest::parse_sdf(s)
    }
}
