use crate::analysis::SDFGraphAnalyzed;
use crate::graph::SDFGraph;
use crate::parasitics::Parasitics;
use crate::subckt::SubcktData;
use crate::types::{BiUnate, PinTrans, SDFCellType, SDFInstance, SDFPin, Transition};
use crate::{instance_name, pin_name_ref};
use miniserde::Deserialize;
use ordered_float::OrderedFloat;
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::fmt::Write;

static CELL_TRANSITION_COMBINATIONS_JSON: &str = include_str!("cells_transition_combinations.json");

// .lib says 5614.3 (calculated from inv1 by calculating delta time over delta capacitance)
// spice sim says 6572.7 ... should investigate
/// Equivalent resistance for a 1um/0.15um PFET (in Ohms)
pub const EQ_RESISTANCE_FOR_100_PFET: f32 = 5614.3;

// .lib says 3326.1 (calculated from inv1 by calculating delta time over delta capacitance)
// spice sim says 2841.4 ... should investigate
/// Equivalent resistance for a 0.65um/0.15um NFET (in Ohms)
pub const EQ_RESISTANCE_FOR_065_NFET: f32 = 3326.1;

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

fn area(w: f32) -> f32 {
    0.15 * w
}

fn perim(w: f32) -> f32 {
    w + 2.0 * 0.15
}

fn pfet(name: &str, d: &str, g: &str, s: &str, w: f32) -> String {
    let bins_pfet = vec![
        0.36, 0.42, 0.54, 0.55, 0.63, 0.64, 0.70, 0.75, 0.79, 0.82, 0.84, 0.86, 0.94, 1.00, 1.12, 1.26, 1.65, 1.68,
        2.00, 3.00, 5.00, 7.00,
    ];

    let pos = bins_pfet
        .binary_search_by(|val| OrderedFloat(*val).cmp(&OrderedFloat(w)))
        .unwrap_or_else(|x| x);
    let closest_bin = bins_pfet[usize::min(pos, bins_pfet.len() - 1)];
    let mult = w / closest_bin;
    let ar = area(w) / mult;
    let pe = perim(w) / mult;

    format!(
        "X{name} {d} {g} {s} Vdd sky130_fd_pr__pfet_01v8_hvt w={:.2} l=0.15 ad={:.2} as={:.2} pd={:.2} ps={:.2} m={:.2}",
        closest_bin, ar, ar, pe, pe, mult
    )
}

fn nfet(name: &str, d: &str, g: &str, s: &str, w: f32) -> String {
    let bins_nfet = vec![
        0.36, 0.39, 0.42, 0.52, 0.54, 0.55, 0.58, 0.6, 0.61, 0.64, 0.65, 0.74, 0.84, 1.0, 1.26, 1.68, 2.0, 3.0, 5.0,
        7.0,
    ];

    let pos = bins_nfet
        .binary_search_by(|val| OrderedFloat(*val).cmp(&OrderedFloat(w)))
        .unwrap_or_else(|x| x);
    let closest_bin = bins_nfet[usize::min(pos, bins_nfet.len() - 1)];
    let mult = w / closest_bin;
    let ar = area(w) / mult;
    let pe = perim(w) / mult;

    format!(
        "X{name} {d} {g} {s} Vgnd sky130_fd_pr__nfet_01v8 w={:.2} l=0.15 ad={:.2} as={:.2} pd={:.2} ps={:.2} m={:.2}",
        closest_bin, ar, ar, pe, pe, mult
    )
}

pub fn extract_spice_for_manual_analysis(
    graph: &SDFGraph,
    analysis: &SDFGraphAnalyzed,
    subckt: &SubcktData,
    parasitics: Option<&Parasitics>,
    output: &PinTrans,
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
    writeln!(&mut spice, "Vclk clk Vgnd PULSE(0 {} 0n 0.2n 0 0 0)", VDD).unwrap();
    writeln!(&mut spice).unwrap();

    let mut values: FxHashMap<_, Cow<str>> = Default::default();
    let mut pins_to_plot = FxHashSet::default();

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

    for (instance, celltype, pin_i, pin_o) in instances.iter() {
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
            //if i == 0 {
            pins_to_plot.insert(out.clone());
            //}
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

        if pin_vals.is_none() && celltype_short != "dfxtp" {
            eprintln!("no pin combination found for {}", celltype);
        }

        writeln!(
            &mut spice,
            "* {} -> {} ({}): {}",
            pin_i.0, pin_o.0, pin_i.1, celltype_short
        )
        .unwrap();

        writeln!(&mut spice, "* pins ").unwrap();
        for pin in &subckt.data[celltype].pins {
            let full_pin = format!("{}/{}", instance, pin);
            if values.contains_key(&**pin) {
                continue;
            }

            let connected_to = &graph.reverse_graph[&(full_pin.clone(), Transition::Rise)][0].dst.0;

            let instance_name = instance_name(connected_to);

            if celltype_short == "dfxtp" {
                writeln!(&mut spice, "V{} {} Vgnd {}", full_pin, full_pin, VDD).unwrap();
                values.insert(pin, full_pin.into());
                continue;
            }
            if let Some(pin_vals) = pin_vals {
                if let Some(celltype_name) = graph.instance_celltype.get(&instance_name) {
                    let drive = subckt.data[celltype_name].output_pin_drive[pin_name_ref(connected_to)];

                    let inv_in_node = format!("inv_in_{}/{}", instance, pin);
                    let inv_in_val = !pin_vals.pins[pin];

                    const FLIP_FLOP_DELAY: f32 = 0.418;
                    const INV_DELAY: f32 = 0.15;
                    const RISE_DELAY: f32 = 0.1;
                    let _t_setup = analysis
                        .max_delay
                        .get(&(
                            connected_to.clone(),
                            if pin_vals.pins[pin] {
                                Transition::Rise
                            } else {
                                Transition::Fall
                            },
                        ))
                        .copied()
                        .unwrap_or_default()
                        + FLIP_FLOP_DELAY
                        - INV_DELAY
                        - RISE_DELAY;

                    /*eprintln!(
                        "{} -> {} ({}): {} -> {} ({}): {}",
                        connected_to,
                        instance_name,
                        pin_name_ref(connected_to),
                        instance,
                        pin,
                        pin_name_ref(pin),
                        t_setup
                    );*/

                    // ignore t_setup for now, don't deal with simultaneous switching as it's weird
                    // to convert STA time to spice time correctly
                    writeln!(
                        &mut spice,
                        "V{} {} Vgnd {}",
                        &inv_in_node,
                        &inv_in_node,
                        if inv_in_val { VDD } else { "0" },
                    )
                    .unwrap();

                    writeln!(
                        &mut spice,
                        "{}\n{}",
                        pfet(&full_pin, &full_pin, &inv_in_node, "Vdd", 0.15 / drive.rise_lw),
                        nfet(&full_pin, &full_pin, &inv_in_node, "Vgnd", 0.15 / drive.fall_lw)
                    )
                    .unwrap();
                } else {
                    if pin_vals.pins[pin] {
                        writeln!(&mut spice, "V{} {} Vgnd {}", full_pin, full_pin, VDD).unwrap();
                    } else {
                        writeln!(&mut spice, "V{} {} Vgnd 0", full_pin, full_pin).unwrap();
                    };
                }
            }

            values.insert(pin, full_pin.into());
        }

        writeln!(&mut spice, "\n* cell ").unwrap();

        subckt.instanciate(instance, celltype, &values, &mut spice);
    }

    // remove output of last instance
    for out in &graph.instance_outs[o_instance] {
        pins_to_plot.remove(out);
    }

    writeln!(&mut spice).unwrap();

    let load_model = &[23.2746, 32.1136, 48.4862, 64.0974, 86.2649, 84.2649];

    let res_base = 0.0745 * 1000.0; // in ohms
    let capa_base = 1.42e-5; // in picofarads
    let slope = 8.36;

    let mut resistances = String::new();
    let mut capacitances = String::new();

    for (i, (pin_in, pin_out)) in wires.iter().enumerate() {
        if let Some(para) = parasitics {
            if let Some(wire) = para.wires.get(&(pin_in.clone(), pin_out.clone())) {
                writeln!(&mut resistances, "RW{} {} {} {}", i, pin_in, pin_out, wire.res).unwrap();
                writeln!(&mut capacitances, "CW{} {} Vgnd {}p", i, pin_out, wire.cap * 1e12).unwrap();
                continue;
            } else {
                eprintln!("No parasitics for wire {} -> {}", pin_in, pin_out);
            }
        }

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

    if let Some(para) = parasitics {
        for (pin, value) in &para.caps {
            writeln!(&mut capacitances, "CW{}_solo {} Vgnd {}p", pin, pin, value * 1e12).unwrap();
        }
    }

    writeln!(&mut spice, "* parasitic wires\n{}\n{}", resistances, capacitances).unwrap();
    writeln!(&mut spice).unwrap();

    let mut to_plot_str = String::new();
    for pin in pins_to_plot {
        write!(to_plot_str, "V({}) ", pin).unwrap();
    }

    writeln!(
        &mut spice,
        r#"
.tran 0.01n 7n
.control
run
plot {}
.endc
.end"#,
        to_plot_str
    )
    .unwrap();

    std::fs::write("out.spice", spice).unwrap();
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
