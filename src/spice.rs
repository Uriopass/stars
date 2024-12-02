use crate::analysis::SDFGraphAnalyzed;
use crate::graph::SDFGraph;
use crate::parasitics::Parasitics;
use crate::subckt::SubcktData;
use crate::types::{BiUnate, PinTrans, SDFCellType, SDFInstance, SDFPin, Transition};
use crate::{instance_name, pin_name, pin_name_ref};
use ordered_float::OrderedFloat;
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::fmt::Write;

static PIN_CAPA_JSON: &str = include_str!("pin_capa.json");

struct PinCapas {
    data: FxHashMap<SDFCellType, f32>,
}

impl PinCapas {
    pub fn new() -> Self {
        Self {
            data: miniserde::json::from_str(PIN_CAPA_JSON).unwrap(),
        }
    }
}

static CELL_TRANSITION_COMBINATIONS_JSON: &str = include_str!("cells_transition_combinations.json");

// .lib says 5614.3 (calculated from inv1 by calculating delta time over delta capacitance)
// spice sim says 6572.7
/// Equivalent resistance for a    1um/0.15um PFET (in Ohms). We premultiply by W / L so we can get actual resistance with R x L / W
pub const EQ_RESISTANCE_PFET_HVT: f32 = 6591.7 * 1.0 / 0.15 / std::f32::consts::LN_2;

// .lib says 3326.1 (calculated from inv1 by calculating delta time over delta capacitance)
// spice sim says 2841.4
/// Equivalent resistance for a 0.65um/0.15um NFET (in Ohms). We premultiply by W / L so we can get actual resistance with R x L / W
pub const EQ_RESISTANCE_NFET: f32 = 2832.4 * 0.65 / 0.15 / std::f32::consts::LN_2;

/// Equivalent capacitance for pfet hvt (in Farads / m²)
pub const CAPA_PER_AREA_PFET_HVT: f32 = 0.00990114 * 1.03;

/// Equivalent capacitance for nfet (in Farads / m²)
pub const CAPA_PER_AREA_NFET: f32 = 0.005819149 * 1.03;

#[derive(Debug, miniserde::Deserialize)]
struct CellTransitionCombination {
    pins: FxHashMap<SDFPin, bool>,
    unate: BiUnate,
}

#[derive(Debug)]
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

pub fn pfet_size(w: f32) -> (f32, f32) {
    static BINS_PFET: &[f32] = &[
        0.36, 0.42, 0.54, 0.55, 0.63, 0.64, 0.70, 0.75, 0.79, 0.82, 0.84, 0.86, 0.94, 1.00, 1.12, 1.26, 1.65, 1.68,
        2.00, 3.00, 5.00, 7.00,
    ];
    let pos = BINS_PFET
        .binary_search_by(|val| OrderedFloat(*val).cmp(&OrderedFloat(w)))
        .unwrap_or_else(|x| x);
    let closest_bin = BINS_PFET[usize::min(pos, BINS_PFET.len() - 1)];
    let mult = w / closest_bin;
    (closest_bin, mult)
}

fn pfet(name: &str, d: &str, g: &str, s: &str, w: f32) -> String {
    let (closest_bin, mult) = pfet_size(w);
    let ar = area(closest_bin) / mult;
    let pe = perim(closest_bin) / mult;

    format!(
        "X{name} {d} {g} {s} Vdd sky130_fd_pr__pfet_01v8_hvt w={:.2} l=0.15 ad={:.2} as={:.2} pd={:.2} ps={:.2} m={:.2}",
        closest_bin, ar, ar, pe, pe, mult
    )
}

pub fn nfet_size(w: f32) -> (f32, f32) {
    static BINS_NFET: &[f32] = &[
        0.36, 0.39, 0.42, 0.52, 0.54, 0.55, 0.58, 0.6, 0.61, 0.64, 0.65, 0.74, 0.84, 1.0, 1.26, 1.68, 2.0, 3.0, 5.0,
        7.0,
    ];
    let pos = BINS_NFET
        .binary_search_by(|val| OrderedFloat(*val).cmp(&OrderedFloat(w)))
        .unwrap_or_else(|x| x);
    let closest_bin = BINS_NFET[usize::min(pos, BINS_NFET.len() - 1)];
    let mult = w / closest_bin;
    (closest_bin, mult)
}

fn nfet(name: &str, d: &str, g: &str, s: &str, w: f32) -> String {
    let (closest_bin, mult) = nfet_size(w);
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
    max_delay: f32,
    path: &[(PinTrans, f32)],
) {
    let transdata = CellTransitionData::new();
    let pincapas = PinCapas::new();

    let mut instances: Vec<(SDFInstance, SDFCellType, PinTrans, PinTrans)> = vec![];
    let mut wires: Vec<(SDFPin, SDFPin)> = Default::default();
    let mut all_pins_in_path = FxHashSet::default();

    let mut last_pin: Option<&PinTrans> = None;

    for (pin, _delay) in path {
        let instance = instance_name(&pin.0);
        let celltype = &graph.instance_celltype[&instance];

        let last_instance = instances.last().map(|v| &v.0);

        all_pins_in_path.insert(pin.0.clone());

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

    let mut shortname_map = FxHashMap::default();

    for (i, instance) in instances.iter().enumerate() {
        shortname_map.insert(&*instance.0, i);
    }

    let shortify = |pin: &str| {
        if let Some((instance, pin)) = pin.rsplit_once('/') {
            if let Some(i) = shortname_map.get(&*instance) {
                return format!("I{}/{}", i, pin);
            }
            return pin.to_string();
        }
        if let Some(i) = shortname_map.get(pin) {
            return format!("I{}", i);
        }
        pin.to_string()
    };

    let mut spice = String::new();

    const VDD: &str = "1.8";

    writeln!(
        &mut spice,
        r#"
* Generated by SDF using stars
* Delay: {:.3}

.title sdf_based_path_extraction_of_{}

.include "./prelude.spice"
Vgnd Vgnd 0 0
Vdd Vdd Vgnd {VDD}
Vclk clk Vgnd PULSE(0 {VDD} 0n 0.2n 0 0 0)

.param v_q_ic = 0
.param v_start = 1.8

.ic V({}) = {{v_q_ic}}

VI0/D I0/D Vgnd {{v_start}}

"#,
        analysis.max_delay[output],
        o_instance,
        shortify(&*instances[0].2 .0)
    )
    .unwrap();

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

    for (i, (instance, celltype, pin_i, pin_o)) in instances.iter().enumerate() {
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
        values.insert(transition_pin, shortify(&pin_i.0).into());

        let mut total_out_capa = 0.0;

        for fanout_pin in &graph.instance_fanout[instance] {
            if all_pins_in_path.contains(fanout_pin) {
                continue;
            }

            let fanout_instance = instance_name(fanout_pin);
            let fanout_celltype = &graph.instance_celltype[&fanout_instance];

            let pin = pin_name_ref(fanout_pin);

            let full = format!("{}/{}", fanout_celltype, pin);
            let Some(capa_v) = pincapas.data.get(&full).copied() else {
                continue;
            };

            total_out_capa += capa_v;
        }

        for out in &graph.instance_outs[instance] {
            values.insert(pin_name_ref(out), shortify(&*out).into());
        }
        pins_to_plot.insert(shortify(&*pin_o.0));

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

        let mut celltype_with_combinations = celltype_short.to_string();

        for (pin, val) in pin_vals.iter().flat_map(|v| v.pins.iter()) {
            celltype_with_combinations.push_str(&format!(", {}={}", shortify(pin), if *val { "1" } else { "0" }));
        }

        writeln!(
            &mut spice,
            "\n* transition {} -> {} ({})\n* celltype {} ",
            pin_i.0, pin_o.0, pin_i.1, celltype_with_combinations
        )
        .unwrap();

        let get_slack = |pintrans: &PinTrans, factor: f32| {
            let t_setup = analysis.max_delay.get(pintrans).copied().map(|v| v / factor);
            let t_arrival = analysis.max_delay_backwards.get(pintrans).copied().map(|v| v / factor);
            if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
                Some(max_delay - (t_setup + t_arrival))
            } else {
                None
            }
        };

        writeln!(&mut spice, "* pins ").unwrap();

        for pin in &subckt.data[celltype].pins {
            let full_pin = format!("{}/{}", instance, pin);
            if values.contains_key(&**pin) {
                continue;
            }

            let connected_to = &graph.reverse_graph[&(full_pin.clone(), Transition::Rise)][0].dst.0;

            let instance_name_ = instance_name(connected_to);

            if celltype_short == "dfxtp" {
                /*writeln!(
                    &mut spice,
                    "V{} {} Vgnd {}",
                    shortify(&*full_pin),
                    shortify(&*full_pin),
                    VDD
                )
                .unwrap();*/
                values.insert(pin, shortify(&*full_pin).into());
                continue;
            }
            if let Some(pin_vals) = pin_vals {
                if let Some(celltype_name) = graph.instance_celltype.get(&instance_name_) {
                    let drive = subckt.data[celltype_name].output_pin_drive[pin_name_ref(connected_to)];

                    let inv_in_node = format!("inv_in_{}/{}", shortify(instance), shortify(pin));
                    let pin_val = pin_vals.pins[pin];
                    let inv_in_val = !pin_vals.pins[pin];

                    const FLIP_FLOP_DELAY: f32 = 0.418;
                    const INV_DELAY: f32 = 0.15;
                    const RISE_DELAY: f32 = 0.1;

                    let connected_to_trans = (
                        connected_to.clone(),
                        if pin_val { Transition::Rise } else { Transition::Fall },
                    );

                    let _t_setup = analysis.max_delay.get(&connected_to_trans).copied().unwrap_or_default()
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

                    let slack_p = get_slack(&(connected_to.clone(), Transition::Rise), 1.2).unwrap_or(0.0); // in ns
                    let slack_n = get_slack(&(connected_to.clone(), Transition::Fall), 1.2).unwrap_or(0.0); // in ns

                    let slack = if inv_in_val { slack_p } else { slack_n } * 1e-9; // in seconds

                    let rd = if pin_val {
                        EQ_RESISTANCE_PFET_HVT * drive.rise_lw
                    } else {
                        EQ_RESISTANCE_NFET * drive.fall_lw
                    };

                    let maxw_p = slack / (rd * 0.15e-6 * CAPA_PER_AREA_PFET_HVT * std::f32::consts::LN_2);
                    let maxw_n = slack / (rd * 0.15e-6 * CAPA_PER_AREA_NFET * std::f32::consts::LN_2);

                    let c_e = graph.instance_fanout[&instance_name_].iter().fold(0.0, |acc, fanout| {
                        if fanout == &full_pin {
                            return acc;
                        }

                        let fanout_instance = instance_name(fanout);
                        let fanout_celltype = &graph.instance_celltype[&fanout_instance];
                        let pin = pin_name_ref(fanout);
                        let full = format!("{}/{}", fanout_celltype, pin);
                        let Some(capa_v) = pincapas.data.get(&full).copied() else {
                            return acc;
                        };
                        acc + capa_v
                    });

                    writeln!(
                        &mut spice,
                        "* maxw_p {}/{} {:.3} slack {} ns",
                        shortify(&*instance),
                        pin,
                        maxw_p * 1e6,
                        slack * 1e9
                    )
                    .unwrap();

                    writeln!(
                        &mut spice,
                        "* maxw_n {}/{} {:.3} slack {} ns",
                        shortify(&*instance),
                        pin,
                        maxw_n * 1e6,
                        slack * 1e9
                    )
                    .unwrap();

                    if c_e > 0.0 {
                        writeln!(
                            &mut spice,
                            "C{}_fanout {} Vgnd {}p",
                            shortify(&*full_pin),
                            shortify(&*full_pin),
                            c_e
                        )
                        .unwrap();
                    }

                    // ignore t_setup for now, don't deal with simultaneous switching as it's weird
                    // to convert STA time to spice time correctly

                    if true {
                        writeln!(
                            &mut spice,
                            "V{} {} Vgnd {}",
                            &inv_in_node,
                            &inv_in_node,
                            if inv_in_val { VDD } else { "0" },
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            &mut spice,
                            "V{} {} Vgnd PULSE({} {} {}n {}n 0 1 2)",
                            &inv_in_node,
                            &inv_in_node,
                            if inv_in_val { "0" } else { VDD },
                            if inv_in_val { VDD } else { "0" },
                            _t_setup,
                            RISE_DELAY * 2.0,
                        )
                        .unwrap();
                    }

                    writeln!(
                        &mut spice,
                        "{}\n{}",
                        pfet(
                            &shortify(&*full_pin),
                            &shortify(&*full_pin),
                            &inv_in_node,
                            "Vdd",
                            0.15 / drive.rise_lw
                        ),
                        nfet(
                            &shortify(&*full_pin),
                            &shortify(&*full_pin),
                            &inv_in_node,
                            "Vgnd",
                            0.15 / drive.fall_lw
                        )
                    )
                    .unwrap();
                } else {
                    if pin_vals.pins[pin] {
                        writeln!(
                            &mut spice,
                            "V{} {} Vgnd {}",
                            &shortify(&*full_pin),
                            &shortify(&*full_pin),
                            VDD
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            &mut spice,
                            "V{} {} Vgnd 0",
                            &shortify(&*full_pin),
                            &shortify(&*full_pin)
                        )
                        .unwrap();
                    };
                }
            }

            values.insert(pin, shortify(&*full_pin).into());
        }

        writeln!(&mut spice, "\n* cell ").unwrap();

        subckt.instanciate(
            &shortify(&*instance),
            celltype,
            &values,
            &mut spice,
            &Default::default(),
        );

        if total_out_capa != 0.0 {
            writeln!(
                &mut spice,
                "C{}_fanout {} Vgnd {}p",
                shortify(&pin_o.0),
                shortify(&pin_o.0),
                total_out_capa
            )
            .unwrap();
        }
    }

    // remove output of last instance
    for out in &graph.instance_outs[o_instance] {
        pins_to_plot.remove(&shortify(&*out));
    }

    writeln!(&mut spice).unwrap();

    let load_model = &[23.2746, 32.1136, 48.4862, 64.0974, 86.2649, 84.2649];

    let res_base = 0.0745 * 10.0; // in ohms
    let capa_base = 1.42e-5; // in picofarads
    let slope = 8.36;

    let mut resistances = String::new();
    let mut capacitances = String::new();

    for (i, (pin_in, pin_out)) in wires.iter().enumerate() {
        if let Some(para) = parasitics {
            if let Some(wire) = para.wires.get(&(pin_in.clone(), pin_out.clone())) {
                writeln!(
                    &mut resistances,
                    "RW{} {} {} {}",
                    i,
                    shortify(pin_in),
                    shortify(pin_out),
                    wire.res
                )
                .unwrap();
                writeln!(
                    &mut capacitances,
                    "CW{} {} Vgnd {}p",
                    i,
                    shortify(pin_out),
                    wire.cap * 1e12
                )
                .unwrap();
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

        writeln!(
            &mut resistances,
            "RW{} {} {} {}",
            i,
            shortify(pin_in),
            shortify(pin_out),
            res
        )
        .unwrap();
        writeln!(&mut capacitances, "CW{} {} Vgnd {}p", i, shortify(pin_out), capa).unwrap();
    }

    if let Some(para) = parasitics {
        for (pin, value) in &para.caps {
            writeln!(
                &mut capacitances,
                "CW{}_solo {} Vgnd {}p",
                shortify(pin),
                shortify(pin),
                value * 1e12
            )
            .unwrap();
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
.tran 0.01n 8n
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
