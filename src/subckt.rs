use crate::types::{SDFCellType, SDFInstance, SDFPin};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::fmt::Write;

pub struct SubcktData {
    pub data: FxHashMap<SDFCellType, Subckt>,
}

#[derive(Debug, Copy, Clone)]
pub struct Drive {
    /// lw = length/width ratio, proportional to resistance (no unit)
    pub rise_lw: f32,
    pub fall_lw: f32,
}

pub struct Load {
    /// in µm², proportional to load capacitance
    pub pfet_area: f32,
    pub nfet_area: f32,
}

pub struct Subckt {
    pub name: String,
    pub pins: Vec<SDFPin>,
    pub temp_variables: Vec<String>,
    body: String,
    pub input_pin_load: FxHashMap<String, Load>,
    pub output_pin_drive: FxHashMap<String, Drive>,
}

impl Subckt {
    pub fn new<'a>(subckt_line: &'a str, lines: &mut impl Iterator<Item = &'a str>) -> Self {
        let mut parts = subckt_line.split_whitespace();
        let _ = parts.next(); // .subckt
        let name = parts.next().unwrap();
        let io_pins: Vec<_> = parts.map(String::from).collect();

        let mut body = String::with_capacity(256);

        #[derive(Copy, Clone, Eq, PartialEq)]
        enum TransistorKind {
            Nfet,
            Pfet,
        }
        #[allow(uncommon_codepoints)]
        struct Transistor<'a> {
            kind: TransistorKind,
            drain: &'a str,
            gate: &'a str,
            source: &'a str,
            w_µm: f32,
            l_µm: f32,
        }

        let mut transistors = Vec::new();

        while let Some(line) = lines.next() {
            if line.starts_with(".ends") {
                break;
            }

            if line.starts_with('X') {
                let mut words = line.split_whitespace();
                let _ = words.next(); // Xtruc
                let drain = words.next().unwrap();
                let gate = words.next().unwrap();
                let source = words.next().unwrap();
                let _ = words.next(); // vpb or vnb
                let kind = if words.next().unwrap().starts_with("sky130_fd_pr__nfet") {
                    TransistorKind::Nfet
                } else {
                    TransistorKind::Pfet
                };

                let mut l_µm = 1.0; // in um
                let mut w_µm = 1.0; // in um

                for word in words {
                    if word.starts_with("w=") {
                        w_µm = word[2..].parse().unwrap();
                    } else if word.starts_with("l=") {
                        l_µm = word[2..].parse().unwrap();
                    }
                }

                transistors.push(Transistor {
                    kind,
                    drain,
                    gate,
                    source,
                    w_µm,
                    l_µm,
                })
            }

            body.push_str(line);
            body.push('\n');
        }

        let mut input_pins = Vec::new();
        let mut output_pins = Vec::new();

        for pin in &io_pins {
            if matches!(&**pin, "VGND" | "VPWR" | "VNB" | "VPB") {
                continue;
            }

            let mut is_input = false;
            let mut is_output = false;

            for transistor in &transistors {
                if transistor.drain == *pin || transistor.source == *pin {
                    is_output = true;
                }
                if transistor.gate == *pin {
                    is_input = true;
                }
            }

            if is_input {
                input_pins.push(pin);
            }

            if is_output {
                output_pins.push(pin);
            }
        }

        let mut input_pin_load = FxHashMap::default();
        let mut output_pin_drive = FxHashMap::default();

        for pin in input_pins {
            let mut in_pfet_area = 0.0;
            let mut in_nfet_area = 0.0;

            for transistor in &transistors {
                if transistor.gate == &**pin {
                    match transistor.kind {
                        TransistorKind::Nfet => {
                            in_nfet_area += transistor.w_µm * transistor.l_µm;
                        }
                        TransistorKind::Pfet => {
                            in_pfet_area += transistor.w_µm * transistor.l_µm;
                        }
                    }
                }
            }

            input_pin_load.insert(
                pin.to_string(),
                Load {
                    pfet_area: in_pfet_area,
                    nfet_area: in_nfet_area,
                },
            );
        }

        let mut pin_wl = FxHashMap::default();
        let mut visited = FxHashSet::default();

        fn calc_wl<'a>(
            pin_wl: &mut FxHashMap<&'a str, f32>,
            visited: &mut FxHashSet<&'a str>,
            transistors: &[Transistor<'a>],
            pin: &'a str,
            kind: TransistorKind,
        ) -> f32 {
            match pin {
                "VGND" | "VPWR" | "VNB" | "VPB" => return 0.0,
                _ => {}
            }
            if pin_wl.contains_key(pin) {
                return pin_wl[pin];
            }
            visited.insert(pin);
            let mut max_lw: f32 = 0.0;
            for transistor in transistors {
                if transistor.kind != kind {
                    continue;
                }
                if transistor.drain == pin {
                    if visited.contains(&transistor.source) {
                        continue;
                    }
                    max_lw = max_lw.max(
                        calc_wl(pin_wl, visited, transistors, &*transistor.source, kind)
                            + transistor.l_µm / transistor.w_µm,
                    );
                }
                if transistor.source == pin {
                    if visited.contains(&transistor.drain) {
                        continue;
                    }
                    max_lw = max_lw.max(
                        calc_wl(pin_wl, visited, transistors, &*transistor.drain, kind)
                            + transistor.l_µm / transistor.w_µm,
                    );
                }
            }
            pin_wl.insert(pin, max_lw);
            max_lw
        }

        for pin in output_pins {
            pin_wl.clear();
            visited.clear();
            let rise_lw = calc_wl(&mut pin_wl, &mut visited, &transistors, &**pin, TransistorKind::Pfet);

            pin_wl.clear();
            visited.clear();
            let fall_lw = calc_wl(&mut pin_wl, &mut visited, &transistors, &**pin, TransistorKind::Nfet);

            output_pin_drive.insert(pin.to_string(), Drive { rise_lw, fall_lw });
        }

        let mut temp_variables_set = FxHashSet::default();
        for transistor in &transistors {
            temp_variables_set.insert(&transistor.drain);
            temp_variables_set.insert(&transistor.gate);
            temp_variables_set.insert(&transistor.source);
        }

        for pin in io_pins.iter() {
            temp_variables_set.remove(&&**pin);
        }

        Subckt {
            name: name.to_string(),
            temp_variables: temp_variables_set.into_iter().map(ToString::to_string).collect(),
            pins: io_pins,
            body,
            input_pin_load,
            output_pin_drive,
        }
    }
}

impl SubcktData {
    pub fn new(contents: &str) -> Self {
        let mut subckt_data = Self {
            data: Default::default(),
        };

        let mut lines = contents.lines();

        while let Some(line) = lines.next() {
            if line.starts_with(".subckt") {
                let subckt = Subckt::new(line, &mut lines);
                subckt_data.data.insert(subckt.name.clone(), subckt);
            }
        }

        subckt_data
    }

    pub fn call(
        &self,
        instance: &SDFInstance,
        celltype: &SDFCellType,
        values: &FxHashMap<&str, Cow<str>>,
        spice_append: &mut String,
    ) {
        let subckt = self.data.get(celltype).unwrap();

        write!(spice_append, "X{} ", instance).unwrap();
        for pin in &subckt.pins {
            let Some(val) = values.get(&**pin) else {
                panic!("Missing value for pin {} for instance {}({})", pin, instance, celltype);
            };
            write!(spice_append, "{} ", val).unwrap();
        }
        writeln!(spice_append, "{}", celltype).unwrap();
    }

    pub fn instanciate(
        &self,
        instance: &SDFInstance,
        celltype: &SDFCellType,
        values: &FxHashMap<&str, Cow<str>>,
        spice_append: &mut String,
    ) {
        let subckt = self.data.get(celltype).unwrap();

        let mut substitutions =
            FxHashMap::with_capacity_and_hasher(subckt.temp_variables.len() + subckt.pins.len(), Default::default());

        for temp_variable in &subckt.temp_variables {
            substitutions.insert(&**temp_variable, format!("{}_{}", instance, temp_variable));
        }

        for pin in &subckt.pins {
            let Some(val) = values.get(&**pin) else {
                panic!("Missing value for pin {} for instance {}({})", pin, instance, celltype);
            };
            substitutions.insert(pin, val.to_string());
        }

        for line in subckt.body.lines() {
            let mut first_word = true;
            for word in line.split_whitespace() {
                if first_word {
                    first_word = false;
                    write!(spice_append, "{}_{} ", word, instance).unwrap();
                    continue;
                }
                if let Some(substitution) = substitutions.get(word) {
                    write!(spice_append, "{} ", substitution).unwrap();
                } else if word == "sky130_fd_pr__special_nfet_01v8" {
                    write!(spice_append, "sky130_fd_pr__nfet_01v8 ").unwrap();
                } else {
                    write!(spice_append, "{} ", word).unwrap();
                }
            }
            writeln!(spice_append).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subckt_data() {
        let contents = r#"
.subckt sky130_fd_sc_hd__and4 a b c y
M1 y a b vdd sky130_fd_sc_hd__nmos
M2 y c b vdd sky130_fd_sc_hd__nmos
M3 y a c vdd sky130_fd_sc_hd__pmos
M4 a_test# a vdd vdd sky130_fd_sc_hd__nmos
.ends"#;

        let subckt_data = SubcktData::new(contents);

        let mut values: FxHashMap<_, _> = Default::default();
        values.insert("a", "oa".into());
        values.insert("b", "ob".into());
        values.insert("c", "oc".into());
        values.insert("y", "oy".into());

        let mut spice = String::new();
        subckt_data.call(
            &"and4_0".to_string(),
            &"sky130_fd_sc_hd__and4".to_string(),
            &values,
            &mut spice,
        );

        let expected = "Xand4_0 oa ob oc oy sky130_fd_sc_hd__and4\n";
        assert_eq!(spice, expected);

        let mut spice = String::new();
        subckt_data.instanciate(
            &"and4_0".to_string(),
            &"sky130_fd_sc_hd__and4".to_string(),
            &values,
            &mut spice,
        );

        let expected = r#"M1_and4_0 oy oa ob vdd sky130_fd_sc_hd__nmos 
M2_and4_0 oy oc ob vdd sky130_fd_sc_hd__nmos 
M3_and4_0 oy oa oc vdd sky130_fd_sc_hd__pmos 
M4_and4_0 and4_0_a_test# oa vdd vdd sky130_fd_sc_hd__nmos 
"#;
        assert_eq!(spice, expected);
    }
}
