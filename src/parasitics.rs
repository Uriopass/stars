use crate::types::SDFPin;
use rustc_hash::FxHashMap;
use spefparse::{ParValue, SPEFHierPortPinRef};
use std::ffi::OsString;

#[derive(Default, Copy, Clone, Debug)]
pub struct ParasitWire {
    /// Ohm
    pub res: f64,
    /// Farad
    pub cap: f64,
}

pub struct Parasitics {
    pub wires: FxHashMap<(SDFPin, SDFPin), ParasitWire>,
    pub caps: FxHashMap<SDFPin, f64>,
}

fn extract_name(pin: SPEFHierPortPinRef) -> SDFPin {
    format!(
        "{}{}{}",
        &*pin.0 .0.first().unwrap(),
        pin.1.map(|x| format!("/{}", x)).unwrap_or_default(),
        pin.2.map(|x| format!("[{}]", x)).unwrap_or_default()
    )
}

impl Parasitics {
    pub fn new(path: &OsString) -> Self {
        let content = std::fs::read_to_string(path).expect("Could not read SPEF file");

        let spef = spefparse::SPEF::parse_str(&content).expect("Could not parse SPEF file");

        let mut me = Self {
            wires: FxHashMap::default(),
            caps: FxHashMap::default(),
        };

        let res_unit = spef.header.res_unit as f64;
        let cap_unit = spef.header.cap_unit as f64;

        for net in spef.nets {
            for wire in net.caps {
                let from = extract_name(wire.a);
                let to = wire.b.map(|b| extract_name(b));
                let ParValue::Single(val) = wire.val else {
                    panic!("Expected single value")
                };
                let val = val as f64 * cap_unit;

                if val == 0.0 {
                    continue;
                }
                match to {
                    Some(to) => {
                        me.wires.entry((from.clone(), to.clone())).or_default().cap = val;
                        me.wires.entry((to, from)).or_default().cap = val;
                    }
                    None => {
                        me.caps.insert(from.clone(), val);
                    }
                }
            }
            for wire in net.ress {
                let from = extract_name(wire.a);
                let to = extract_name(wire.b);
                let ParValue::Single(val) = wire.val else {
                    panic!("Expected single value")
                };
                me.wires.entry((from, to)).or_default().res = val as f64 * res_unit;
            }
        }

        me
    }
}
