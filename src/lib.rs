#![allow(uncommon_codepoints)]

pub mod analysis;
pub mod graph;
pub mod html;
pub mod parasitics;
pub mod spice;
pub mod subckt;
pub mod types;

use types::SDFPin;

/// Extract the name of the pin from the full path.
/// For example, `and4/A` -> `A`
pub fn pin_name_ref(pin: &SDFPin) -> &str {
    let Some(v) = pin.rsplit_once('/') else {
        return pin;
    };
    v.1
}

/// Extract the name of the pin from the full path.
/// For example, `and4/A` -> `A`
pub fn pin_name(pin: &SDFPin) -> String {
    let Some(v) = pin.rsplit_once('/') else {
        return pin.to_string();
    };
    v.1.to_string()
}

/// Extract the name of the instance from the full path.
/// For example, `and4/A` -> `and4`
pub fn instance_name(pin: &SDFPin) -> String {
    let Some(v) = pin.rsplit_once('/') else {
        return pin.to_string();
    };
    v.0.to_string()
}

/// Turns sky130_fd_sc_hd__xor2_1 into xor2
pub fn celltype_short(celltype: &str) -> &str {
    celltype
        .trim_start_matches("sky130_fd_sc_hd__")
        .rsplit_once('_')
        .unwrap()
        .0
}

pub fn celltype_short_with_size(celltype: &str) -> &str {
    celltype.trim_start_matches("sky130_fd_sc_hd__")
}
