use miniserde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::ops::Neg;

/// Example: instance3/A
pub type SDFPin = String;
/// Example: instance3
pub type SDFInstance = String;
/// Example: sky130_fd_sc_hd__xor2_1
pub type SDFCellType = String;
/// Example: (instance1/A, rise)
pub type PinTrans = (SDFPin, Transition);

pub type PinMap<V> = BTreeMap<SDFPin, V>;
pub type PinTransMap<V> = BTreeMap<PinTrans, V>;
pub type PinSet = BTreeSet<SDFPin>;
pub type PinTransSet = BTreeSet<PinTrans>;
pub type InstanceMap<V> = BTreeMap<SDFInstance, V>;

#[derive(Debug, Deserialize)]
pub enum TriUnate {
    #[serde(rename = "positive_unate")]
    Positive,
    #[serde(rename = "negative_unate")]
    Negative,
    #[serde(rename = "non_unate")]
    Non,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize, PartialOrd, Ord)]
pub enum Transition {
    /// 0 -> 1
    #[serde(rename = "rise")]
    Rise,
    /// 1 -> 0
    #[serde(rename = "fall")]
    Fall,
}

impl Display for Transition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Transition::Rise => write!(f, "↗"),
            Transition::Fall => write!(f, "↘"),
        }
    }
}

impl Neg for Transition {
    type Output = Transition;

    fn neg(self) -> Self::Output {
        match self {
            Transition::Rise => Transition::Fall,
            Transition::Fall => Transition::Rise,
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
pub enum BiUnate {
    #[serde(rename = "positive")]
    Positive,
    #[serde(rename = "negative")]
    Negative,
}
