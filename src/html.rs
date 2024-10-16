use crate::{
    instance_name, pin_name,  PinSet, SDFGraph, SDFGraphAnalyzed,
    SDFInstance, SDFPin, Transition,
};
use std::fmt::Write;

pub fn extract_html_for_manual_analysis(
    graph: &SDFGraph,
    analysis: &SDFGraphAnalyzed,
    output: &SDFPin,
    max_delay: f32,
    path: &[(SDFPin, Transition, f32)],
) {
    let mut instances: Vec<(SDFInstance, SDFPin, Option<Transition>)> = vec![];
    let mut pins_in_path: PinSet = Default::default();

    let mut last_pin: Option<&SDFPin> = None;
    for (pin, transition, _delay) in path {
        let instance = instance_name(pin);
        let last_instance = instances.last().map(|v| &v.0);

        if last_instance == Some(&instance) {
            last_pin = Some(pin);
            continue;
        }

        if let Some(last_pin) = last_pin {
            pins_in_path.insert(last_pin.clone());
        }
        pins_in_path.insert(pin.clone());

        instances.push((instance.clone(), pin.clone(), Some(*transition)));

        last_pin = Some(pin);
    }

    let o_instance = output.rsplit_once('/').unwrap().0;

    instances.push((o_instance.to_string(), output.clone(), None));
    pins_in_path.insert(output.clone());
    pins_in_path.insert(last_pin.unwrap().clone());

    let mut html = String::new();
    html.push_str(r#"<html lang="en">
<head>
<meta charset="UTF-8">
<style>
    table, th, td { border: 1px solid #c1c1c1; border-collapse: collapse; }
    th, td { padding: 5px 10px; }
    td {
    font-family: monospace;
    text-align: right;
    }
</style>
<title>Path analysis</title>
</head>
<body>
    <table>
    <tr>
        <th>Instance</th>
        <th>Celltype</th>
        <th>Setup</th>
        <th>Arr.</th>
        <th><b>Slack</b></th>
        <th></th>
        <th>Input Pin: Setup, Arr, <b>Slack</b></th>
        <th>Output Cells Pin (fanout)</th>
    </tr>"#);

    for (instance, wire_in, transition) in &instances {
        let celltype = graph.instance_celltype[instance].trim_start_matches("sky130_fd_sc_hd__");
        let mut pin_out = graph.instance_outs[instance].first().unwrap();
        let pin_out_holder = String::new();
        if !pins_in_path.contains(pin_out) {
            pin_out = &pin_out_holder;
        }

        let mut t_setup = analysis.max_delay.get(pin_out).copied();
        let mut t_arrival = analysis.max_delay_backwards.get(pin_out).copied();
        let mut slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
            Some(max_delay - (t_setup + t_arrival))
        } else {
            None
        };

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
            pin_name(wire_in),
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
            if wire_in == pin_in {
                continue;
            }
            if pin_name(pin_in) == "CLK" {
                continue;
            }
            let t_setup = *analysis.max_delay.get(pin_in).unwrap_or(&f32::NAN);
            let t_arrival = *analysis.max_delay_backwards.get(pin_in).unwrap_or(&f32::NAN);
            let slack = max_delay - (t_setup + t_arrival);

            if !slack.is_nan() {
                write!(
                    input_pin_html,
                    "{}: {:.3} {:.3} <b>{:.3}</b><br>",
                    pin_name(pin_in),
                    t_setup,
                    t_arrival,
                    slack
                )
                .unwrap();
            }
        }
        writeln!(&mut html, "<td>{}</td>", input_pin_html).unwrap();

        let mut output_pin_html = String::new();
        for fanout_pin_in in &graph.instance_fanout[instance] {
            if pins_in_path.contains(fanout_pin_in) {
                continue;
            }

            let instance = instance_name(fanout_pin_in);
            let celltype = &graph.instance_celltype[&instance];
            let celltype_short = celltype
                .trim_start_matches("sky130_fd_sc_hd__");

            let t_setup = analysis.max_delay.get(fanout_pin_in).copied();
            let t_arrival = analysis.max_delay_backwards.get(fanout_pin_in).copied();
            let slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
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
}
