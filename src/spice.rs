use crate::{instance_name, pin_name_ref, PinTrans, SDFCellType, SDFGraph, SDFGraphAnalyzed, SDFInstance, SDFPin};
use miniserde::Deserialize;
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::fmt::Write;

static CELL_TRANSITION_COMBINATIONS_JSON: &str = include_str!("cells_transition_combinations.json");

#[derive(Debug, Deserialize, Eq, PartialEq)]
enum BiUnate {
    #[serde(rename = "positive")]
    Positive,
    #[serde(rename = "negative")]
    Negative,
}

#[derive(Debug, Deserialize)]
struct CellTransitionCombination {
    pins: FxHashMap<SDFPin, bool>,
    unate: BiUnate,
}

#[derive(Debug, Deserialize)]
pub struct CellTransitionData {
    data: FxHashMap<SDFCellType, FxHashMap<SDFPin, Vec<CellTransitionCombination>>>,
}

impl CellTransitionData {
    pub fn new() -> Self {
        Self {
            data: miniserde::json::from_str(CELL_TRANSITION_COMBINATIONS_JSON).unwrap(),
        }
    }
}

pub struct SubcktData {
    data: FxHashMap<SDFCellType, Subckt>,
}

struct Subckt {
    pins: Vec<SDFPin>,
    temp_variables: Vec<String>,
    body: String,
}

impl SubcktData {
    pub fn new(contents: &str) -> Self {
        let mut subckt_data = Self {
            data: Default::default(),
        };

        let mut lines = contents.lines();

        while let Some(line) = lines.next() {
            if line.starts_with(".subckt") {
                let mut parts = line.split_whitespace();
                let _ = parts.next(); // .subckt
                let name = parts.next().unwrap();
                let pins = parts.map(String::from).collect();

                let mut body = String::with_capacity(256);
                let mut temp_variables = FxHashSet::default();

                while let Some(line) = lines.next() {
                    if line.starts_with(".ends") {
                        break;
                    }

                    for word in line.split_whitespace() {
                        if word.starts_with("a_") && word.ends_with('#') {
                            temp_variables.insert(word.to_string());
                        }
                    }

                    body.push_str(line);
                    body.push('\n');
                }

                subckt_data.data.insert(
                    name.to_string(),
                    Subckt {
                        pins,
                        temp_variables: temp_variables.into_iter().collect(),
                        body,
                    },
                );
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

#[allow(unreachable_code, dead_code, unused_variables)]
pub fn extract_spice_for_manual_analysis(
    graph: &SDFGraph,
    analysis: &SDFGraphAnalyzed,
    subckt: &SubcktData,
    output: &PinTrans,
    max_delay: f32,
    path: &[(PinTrans, f32)],
) {
    let transdata = CellTransitionData::new();

    let mut instances: Vec<(SDFInstance, SDFCellType, PinTrans, PinTrans)> = vec![];
    let mut wires: Vec<(SDFPin, SDFPin)> = Default::default();

    let mut last_pin: Option<&PinTrans> = None;
    for (pin, _delay) in path {
        let instance = instance_name(&pin.0);
        let celltype = &graph.instance_celltype[&instance];

        let last_instance = instances.last().map(|v| &v.0);

        if last_instance == Some(&instance) {
            instances.last_mut().unwrap().3 = pin.clone();
            last_pin = Some(pin);
            continue;
        }

        if let Some(last_pin) = last_pin {
            wires.push((last_pin.0.clone(), pin.0.clone()));
        }
        instances.push((instance.clone(), celltype.clone(), pin.clone(), pin.clone()));

        last_pin = Some(pin);
    }

    let o_instance = output.0.rsplit_once('/').unwrap().0;
    let o_celltype = &graph.instance_celltype[o_instance];

    instances.push((
        o_instance.to_string(),
        o_celltype.clone(),
        output.clone(),
        output.clone(),
    ));
    wires.push((last_pin.unwrap().0.clone(), output.0.clone()));

    let mut spice = String::new();

    const VDD: &str = "1.8";

    writeln!(&mut spice, "* Generated by SDF using stars").unwrap();
    writeln!(&mut spice, "* Delay: {:.3}", analysis.max_delay[output]).unwrap();
    writeln!(&mut spice).unwrap();
    writeln!(&mut spice, ".title sdf_based_path_extraction_of_{}", o_instance).unwrap();
    writeln!(&mut spice).unwrap();
    writeln!(&mut spice, ".include \"./lib/prelude.spice\"").unwrap();
    writeln!(&mut spice, "Vgnd Vgnd 0 0").unwrap();
    writeln!(&mut spice, "Vdd Vdd Vgnd {}", VDD).unwrap();
    writeln!(&mut spice, "Vclk clk Vgnd PULSE(0 {} 0n 0.2n 0.2n 4.6n 10.0n)", VDD).unwrap();
    writeln!(&mut spice).unwrap();

    let mut values: FxHashMap<_, Cow<str>> = Default::default();
    let mut pins_to_plot = FxHashSet::default();

    let mut const_pin: FxHashMap<_, _> = Default::default();

    /*
    let mut celltypes = FxHashSet::default();
    for (_, celltype, pin) in &instances {
        celltypes.insert((&**celltype, pin_name_ref(pin)));
    }
    for (celltype, pin) in &celltypes {
        let celltype_short = celltype
            .trim_start_matches("sky130_fd_sc_hd__")
            .rsplit_once('_')
            .unwrap()
            .0;

        let other_pins = subckt.data[*celltype]
            .pins
            .iter()
            .filter(|p| p != pin && *p != "VPWR" && *p != "VPB" && *p != "VGND" && *p != "VNB")
            .collect::<Vec<_>>();
        eprintln!(
            "Using celltype/pin: {}/{} (other pins: {:?})",
            celltype_short, pin, other_pins
        );
    }*/

    for (instance, celltype, pin_i, pin_o) in &instances {
        let celltype_short = celltype
            .trim_start_matches("sky130_fd_sc_hd__")
            .rsplit_once('_')
            .unwrap()
            .0;
        values.clear();

        values.insert("VGND", "Vgnd".into());
        values.insert("VNB", "Vgnd".into());
        values.insert("VPB", "Vdd".into());
        values.insert("VPWR", "Vdd".into());
        values.insert("CLK", "clk".into());
        values.insert("RESET_B", "Vdd".into()); // reset really is nreset (damnit)

        let transition_pin = pin_name_ref(&pin_i.0); // instance/A -> A
        values.insert(transition_pin, pin_i.0.clone().into());

        for out in &graph.instance_outs[instance] {
            values.insert(pin_name_ref(out), out.into());
            pins_to_plot.insert(out);
        }

        let unate = if pin_i.1 == pin_o.1 {
            BiUnate::Positive
        } else {
            BiUnate::Negative
        };

        let pin_vals = transdata
            .data
            .get(celltype)
            .and_then(|v| v.get(transition_pin))
            .map(|v| v.iter().find(|v| v.unate == unate).expect("No transition found"));

        for pin in &subckt.data[celltype].pins {
            if values.contains_key(&**pin) {
                continue;
            }
            let other_pin = pin_name_ref(pin);
            let full_pin = format!("{}/{}", instance, pin);
            let mut pin_v = "0";

            if let Some(pin_vals) = pin_vals {
                pin_v = if pin_vals.pins[pin] { VDD } else { "0" };
                if celltype_short == "dfxtp" {
                    pin_v = VDD;
                }
            }
            const_pin.insert(full_pin.clone(), pin_v);
            values.insert(pin, full_pin.into());
        }

        subckt.instanciate(instance, celltype, &values, &mut spice);
    }

    // remove output of last instance
    for out in &graph.instance_outs[o_instance] {
        pins_to_plot.remove(out);
    }

    writeln!(&mut spice).unwrap();

    for (pin, value) in &const_pin {
        writeln!(&mut spice, "V{} {} Vgnd {}", pin, pin, value).unwrap();
    }

    writeln!(&mut spice).unwrap();

    let load_model = &[23.2746, 32.1136, 48.4862, 64.0974, 86.2649, 84.2649];

    let res_base = 0.0745 * 1000.0; // in ohms
    let capa_base = 1.42e-5; // in picofarads
    let slope = 8.36;

    let mut resistances = String::new();
    let mut capacitances = String::new();

    for (i, (pin_in, pin_out)) in wires.iter().enumerate() {
        let instance_in = instance_name(pin_in);
        let fanout = graph.instance_fanout[&instance_in].len();

        let mult = if fanout <= load_model.len() {
            load_model[fanout - 1]
        } else {
            load_model[load_model.len() - 1] + slope * (fanout as f32 - load_model.len() as f32)
        };

        let res = res_base * mult;
        let capa = capa_base * mult + 0.002 * fanout as f32;

        writeln!(&mut resistances, "RW{} {} {} {}", i, pin_in, pin_out, res).unwrap();
        writeln!(&mut capacitances, "CW{} {} Vgnd {}p", i, pin_out, capa).unwrap();
    }

    writeln!(&mut spice, "{}", resistances).unwrap();
    writeln!(&mut spice, "{}", capacitances).unwrap();

    writeln!(&mut spice).unwrap();

    let mut to_plot_str = String::new();
    for pin in pins_to_plot {
        write!(to_plot_str, "V({}) ", pin).unwrap();
    }

    writeln!(
        &mut spice,
        r#"
.tran 0.01n 10n
.control
run
plot V(clk) {}
.endc
.end"#,
        to_plot_str
    )
    .unwrap();

    std::fs::write("out.spice", spice).unwrap();
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

#[allow(dead_code)]
mod cell_logic {
    fn xnor2(a: bool, b: bool) -> bool {
        !(a ^ b)
    }

    fn dfxtp(d: bool) -> bool {
        d
    }

    fn dfrtp(d: bool) -> bool {
        d
    }

    fn a21o(a1: bool, a2: bool, b1: bool) -> bool {
        (a1 && a2) || b1
    }

    fn a41o(a1: bool, a2: bool, a3: bool, a4: bool, b1: bool) -> bool {
        (a1 && a2 && a3 && a4) || b1
    }

    fn xor2(a: bool, b: bool) -> bool {
        a ^ b
    }

    fn nor2(a: bool, b: bool) -> bool {
        !(a || b)
    }

    fn mux2(a0: bool, a1: bool, s: bool) -> bool {
        if s {
            a1
        } else {
            a0
        }
    }

    fn a211o(a1: bool, a2: bool, b1: bool, c1: bool) -> bool {
        (a1 && a2) || b1 || c1
    }

    fn a22o(a1: bool, a2: bool, b1: bool, b2: bool) -> bool {
        (a1 && a2) || (b1 && b2)
    }

    fn o211a(a1: bool, a2: bool, b1: bool, c1: bool) -> bool {
        (a1 || a2) && b1 && c1
    }

    fn a21oi(a1: bool, a2: bool, b1: bool) -> bool {
        !((a1 && a2) || b1)
    }

    fn a311o(a1: bool, a2: bool, a3: bool, b1: bool, c1: bool) -> bool {
        (a1 && a2 && a3) || b1 || c1
    }

    fn nand2b(a_n: bool, b: bool) -> bool {
        !(!a_n && b)
    }

    fn o21a(a1: bool, a2: bool, b1: bool) -> bool {
        (a1 || a2) && b1
    }

    fn clkbuf(a: bool) -> bool {
        a
    }

    fn and2(a: bool, b: bool) -> bool {
        a && b
    }

    fn buf(a: bool) -> bool {
        a
    }

    fn a221oi(a1: bool, a2: bool, b1: bool, b2: bool, c1: bool) -> bool {
        !((a1 && a2) || (b1 && b2) || c1)
    }

    fn or4(a: bool, b: bool, c: bool, d: bool) -> bool {
        a || b || c || d
    }
}
