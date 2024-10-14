use crate::{
    instance_name, pin_name, pin_name_ref, InstanceMap, PinMap, PinSet, SDFCellType, SDFGraph, SDFGraphAnalyzed,
    SDFInstance, SDFPin, Transition,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Write;

pub struct SubcktData {
    data: FxHashMap<SDFCellType, Subckt>,
}

struct Subckt {
    pins: Vec<SDFPin>,
}

impl SubcktData {
    pub fn new(contents: &str) -> Self {
        let mut subckt_data = Self {
            data: Default::default(),
        };

        for line in contents.lines() {
            if line.starts_with(".subckt") {
                let mut parts = line.split_whitespace();
                let _ = parts.next(); // .subckt
                let name = parts.next().unwrap();
                let pins = parts.map(String::from).collect();
                subckt_data.data.insert(name.to_string(), Subckt { pins });
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
}

#[allow(unreachable_code, dead_code, unused_variables)]
pub fn extract_spice_for_manual_analysis(
    graph: &SDFGraph,
    analysis: &SDFGraphAnalyzed,
    subckt: &SubcktData,
    output: &SDFPin,
    max_delay: f32,
    path: &[(SDFPin, Transition, f32)],
) {
    let mut instances: Vec<(SDFInstance, SDFCellType)> = vec![];
    let mut wires: Vec<(SDFPin, SDFPin)> = Default::default();
    let mut pins_in_path: PinSet = Default::default();
    let mut wire_in: InstanceMap<SDFPin> = Default::default();
    let mut arrivals: PinMap<_> = Default::default();
    let mut constraints: PinMap<_> = Default::default();
    let mut transitions: PinMap<Transition> = Default::default();

    let mut last_pin: Option<&SDFPin> = None;
    for (pin, transition, _delay) in path {
        constraints.remove(pin);

        let instance = &graph.pin_instance[pin];
        let celltype = &graph.instance_celltype[instance];

        let last_instance = instances.last().map(|v| &v.0);

        if last_instance != Some(instance) {
            if let Some(last_pin) = last_pin {
                wire_in.insert(instance.clone(), pin.clone());
                wires.push((last_pin.clone(), pin.clone()));
                pins_in_path.insert(last_pin.clone());
                pins_in_path.insert(pin.clone());
            }
            // external conn
            instances.push((instance.clone(), celltype.clone()));
        } else {
            // internal conn
            transitions.insert(pin.clone(), *transition);
            for pin_in in &graph.instance_ins[instance] {
                if pin_in == pin {
                    eprintln!("weird...");
                    continue;
                }

                let t_setup = *analysis.max_delay.get(pin_in).unwrap_or(&0.0);
                let t_arrival = *analysis.max_delay_backwards.get(pin_in).unwrap_or(&0.0);
                let slack = max_delay - (t_setup + t_arrival);

                arrivals.insert(pin_in.clone(), (t_setup, t_arrival, slack));
            }
            for pin_in in &graph.instance_fanout[instance] {
                let t_setup = *analysis.max_delay.get(pin_in).unwrap_or(&0.0);
                let t_arrival = *analysis.max_delay_backwards.get(pin_in).unwrap_or(&0.0);
                let slack = max_delay - (t_setup + t_arrival);
                constraints.insert(pin_in.clone(), (t_setup, t_arrival, slack));
            }
        }

        last_pin = Some(pin);
    }

    let o_instance = output.rsplit_once('/').unwrap().0;
    let o_celltype = &graph.instance_celltype[o_instance];

    constraints.remove(output);
    instances.push((o_instance.to_string(), o_celltype.clone()));
    wires.push((last_pin.unwrap().clone(), output.clone()));
    pins_in_path.insert(output.clone());
    pins_in_path.insert(last_pin.unwrap().clone());
    wire_in.insert(o_instance.to_string(), output.clone());

    let mut html = String::new();
    writeln!(&mut html, "<html>").unwrap();
    writeln!(&mut html, "<head>").unwrap();
    // utf8
    writeln!(&mut html, "<meta charset=\"UTF-8\">").unwrap();
    writeln!(&mut html, "<style>").unwrap();
    html.push_str(
        r#"
table, th, td { border: 1px solid #c1c1c1; border-collapse: collapse; }
th, td { padding: 5px 10px; }
td {
    font-family: monospace;
    text-align: right;
}
        "#,
    );
    writeln!(&mut html, "</style>").unwrap();
    writeln!(&mut html, "</head>").unwrap();
    writeln!(&mut html, "<body>").unwrap();
    writeln!(&mut html, "<table>").unwrap();
    writeln!(&mut html, "<tr>").unwrap();
    writeln!(&mut html, "<th>Instance</th>").unwrap();
    writeln!(&mut html, "<th>Celltype</th>").unwrap();
    writeln!(&mut html, "<th>Setup</th>").unwrap();
    writeln!(&mut html, "<th>Arr.</th>").unwrap();
    writeln!(&mut html, "<th><b>Slack</b></th>").unwrap();
    writeln!(&mut html, "<th></th>").unwrap();
    writeln!(&mut html, "<th>Input Pin: Setup, Arr, <b>Slack</b></th>").unwrap();
    writeln!(&mut html, "<th>Output Cells Pin (fanout)</th>").unwrap();
    writeln!(&mut html, "</tr>").unwrap();

    for (instance, celltype) in &instances {
        let celltype = graph.instance_celltype[instance].trim_start_matches("sky130_fd_sc_hd__");
        let mut pin_out = graph.instance_outs[instance].first().unwrap();
        let pin_out_holder = String::new();
        if !pins_in_path.contains(pin_out) {
            pin_out = &pin_out_holder;
        }
        let wire_in = wire_in.get(instance);

        let mut t_setup = analysis.max_delay.get(pin_out).copied();
        let mut t_arrival = analysis.max_delay_backwards.get(pin_out).copied();
        let mut slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
            Some(max_delay - (t_setup + t_arrival))
        } else {
            None
        };
        let transition = transitions.get(pin_out).copied();

        if instance == &instance_name(output) {
            t_setup = None;
            t_arrival = None;
            slack = None;
        }

        writeln!(&mut html, "<tr>").unwrap();
        writeln!(
            &mut html,
            "<td><center>{}<br/>{} → {}</center></td>",
            instance,
            pin_name(wire_in.unwrap_or(&String::new())),
            pin_name(pin_out)
        )
        .unwrap();
        writeln!(&mut html, "<td>{}</td>", celltype).unwrap();
        let mut writecell = |v: Option<f32>| {
            if let Some(v) = v {
                writeln!(&mut html, "<td>{:.3}</td>", v).unwrap();
            } else {
                writeln!(&mut html, "<td></td>").unwrap();
            }
        };
        writecell(t_setup);
        writecell(t_arrival);
        writecell(slack);
        if let Some(transition) = transition {
            writeln!(&mut html, "<td>{}</td>", transition).unwrap();
        } else {
            writeln!(&mut html, "<td></td>").unwrap();
        }

        let mut input_pin_html = String::new();
        for pin_in in &graph.instance_ins[instance] {
            if wire_in == Some(pin_in) {
                continue;
            }
            if pin_name(pin_in) == "CLK" {
                continue;
            }
            if let Some((t_setup, t_arrival, slack)) = arrivals.get(pin_in).copied() {
                write!(
                    input_pin_html,
                    "{}: {:.3} {:.3} <b>{:.3}</b><br>",
                    pin_name(pin_in),
                    t_setup,
                    t_arrival,
                    slack
                )
                .unwrap();
            } else {
                write!(input_pin_html, "{}<br>", pin_in).unwrap();
            }
        }
        writeln!(&mut html, "<td>{}</td>", input_pin_html).unwrap();

        let mut output_pin_html = String::new();
        for fanout_pin_in in &graph.instance_fanout[instance] {
            if pins_in_path.contains(fanout_pin_in) {
                continue;
            }

            let instance = &graph.pin_instance[fanout_pin_in];
            let celltype = &graph.instance_celltype[instance];
            let celltype_short = celltype
                .trim_start_matches("sky130_fd_sc_hd__")
                .rsplit_once('_')
                .unwrap()
                .0;

            let mut t_setup = analysis.max_delay.get(fanout_pin_in).copied();
            let mut t_arrival = analysis.max_delay_backwards.get(fanout_pin_in).copied();
            let mut slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
                Some(max_delay - (t_setup + t_arrival))
            } else {
                None
            };

            if let (Some(t_setup), Some(t_arrival), Some(slack)) = (t_setup, t_arrival, slack) {
                write!(
                    output_pin_html,
                    "{}.{}: {:.3} {:.3} <b>{:.3}</b><br>",
                    celltype_short,
                    pin_name(fanout_pin_in),
                    t_setup,
                    t_arrival,
                    slack
                )
                .unwrap();
            } else {
                write!(output_pin_html, "{}.{}<br>", celltype_short, pin_name(fanout_pin_in)).unwrap();
            }
        }
        writeln!(&mut html, "<td>{}</td>", output_pin_html).unwrap();

        writeln!(&mut html, "</tr>").unwrap();
    }

    writeln!(&mut html, "</table>").unwrap();
    writeln!(&mut html, "</body>").unwrap();
    writeln!(&mut html, "</html>").unwrap();

    std::fs::write("path.html", html).unwrap();

    return;

    let mut spice = String::new();

    let subckt_file = std::env::var("SUBCKT_FILE");

    const VDD: &str = "1.8";

    writeln!(&mut spice, "* Generated by SDF using stars").unwrap();
    writeln!(&mut spice, "* Delay: {:.3}", analysis.max_delay[output]).unwrap();
    writeln!(&mut spice).unwrap();
    writeln!(&mut spice, ".title sdf_based_path_extraction_of_{}", o_instance).unwrap();
    writeln!(&mut spice).unwrap();
    writeln!(&mut spice, ".include {}", subckt_file.unwrap()).unwrap();
    writeln!(&mut spice).unwrap();
    writeln!(&mut spice, "Vgnd Vgnd 0 0").unwrap();
    writeln!(&mut spice, "Vdd Vdd Vgnd {}", VDD).unwrap();
    writeln!(&mut spice, "Vclk clk Vgnd PULSE(0 {} 0n 0.2n 0.2n 4.6n 10.0n)", VDD).unwrap();
    writeln!(&mut spice).unwrap();

    let celltypes = instances.iter().map(|(_, celltype)| celltype).collect::<BTreeSet<_>>();
    for celltype in celltypes {
        let celltype_short = celltype
            .trim_start_matches("sky130_fd_sc_hd__")
            .rsplit_once('_')
            .unwrap()
            .0;
        writeln!(
            &mut spice,
            ".include ./sky130_fd_sc_hd/cells/{}/{}.spice",
            celltype_short, celltype
        )
        .unwrap();
    }
    writeln!(&mut spice).unwrap();

    let mut values: FxHashMap<_, Cow<str>> = Default::default();
    let mut pins_to_plot = FxHashSet::default();

    let mut const_pin: FxHashMap<_, _> = FxHashMap::default();

    for (instance, celltype) in &instances {
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

        if let Some(pin_i) = wire_in.get(instance) {
            values.insert(pin_name_ref(pin_i), pin_i.into());
        }
        for out in &graph.instance_outs[instance] {
            values.insert(pin_name_ref(out), out.into());
            pins_to_plot.insert(out);
        }

        for pin in &subckt.data[celltype].pins {
            if values.contains_key(&**pin) {
                continue;
            }
            let full_pin = format!("{}/{}", instance, pin);
            let pin_v = match celltype_short {
                "dfrtp" | "dfxtp" => VDD,
                "and4" => VDD,
                _ => "0.0",
            };
            const_pin.insert(full_pin.clone(), pin_v);
            values.insert(pin, full_pin.into());
        }

        subckt.call(instance, celltype, &values, &mut spice);
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
        let instance_in = &graph.pin_instance[pin_in];
        let fanout = graph.instance_fanout[instance_in].len();

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

    println!("\n\n{}", spice);
}
