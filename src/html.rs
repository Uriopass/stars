use crate::{instance_name, pin_name, PinTrans, PinTransSet, SDFGraph, SDFGraphAnalyzed, SDFInstance, Transition};
use ordered_float::OrderedFloat;
use std::fmt::Write;

pub fn extract_html_for_manual_analysis(
    graph: &SDFGraph,
    analysis: &SDFGraphAnalyzed,
    output: &PinTrans,
    max_delay: f32,
    path: &[(PinTrans, f32)],
) {
    let mut instances: Vec<(SDFInstance, PinTrans, PinTrans)> = vec![];
    let mut pins_in_path: PinTransSet = Default::default();

    let mut last_pin: Option<&PinTrans> = None;
    for (pin_t, _delay) in path {
        let instance = instance_name(&pin_t.0);
        let last_instance = instances.last().map(|v| &v.0);

        pins_in_path.insert(pin_t.clone());
        if last_instance == Some(&instance) {
            last_pin = Some(pin_t);
            instances.last_mut().unwrap().2 = pin_t.clone();
            continue;
        }

        instances.push((instance.clone(), pin_t.clone(), pin_t.clone()));

        last_pin = Some(pin_t);
    }

    let o_instance = output.0.rsplit_once('/').unwrap().0;

    instances.push((o_instance.to_string(), output.clone(), output.clone()));
    pins_in_path.insert(output.clone());
    pins_in_path.insert(last_pin.unwrap().clone());

    let mut html = String::new();
    html.push_str(
        r#"<html lang="en">
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
    </tr>"#,
    );

    for (instance, pin_in, pin_out) in &instances {
        let mut pin_out = pin_out;
        let celltype = graph.instance_celltype[instance].trim_start_matches("sky130_fd_sc_hd__");
        let pin_out_holder = (String::new(), Transition::Rise);
        if !pins_in_path.contains(&pin_out) {
            pin_out = &pin_out_holder;
        }

        let mut t_setup = analysis.max_delay.get(&pin_out).copied();
        let mut t_arrival = analysis.max_delay_backwards.get(&pin_out).copied();
        let mut slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
            Some(max_delay - (t_setup + t_arrival))
        } else {
            None
        };

        if instance == &instance_name(&output.0) {
            t_setup = None;
            t_arrival = None;
            slack = None;
        }

        writeln!(&mut html, "<tr>").unwrap();
        writeln!(
            &mut html,
            "<td><center>{}<br/>{} → {}</center></td>",
            instance,
            pin_name(&pin_in.0),
            pin_name(&pin_out.0)
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
        writeln!(
            &mut html,
            "<td>{}{}<br />{}{}</td>",
            pin_in.0, pin_in.1, pin_out.0, pin_out.1
        )
        .unwrap();

        let mut input_pin_html = String::new();
        for other_pin_in in &graph.instance_ins[instance] {
            for transition in [Transition::Rise, Transition::Fall] {
                let other_pin_in = (other_pin_in.clone(), transition);
                if pin_in == &other_pin_in {
                    continue;
                }
                if pin_name(&other_pin_in.0) == "CLK" {
                    continue;
                }
                let t_setup = *analysis.max_delay.get(&other_pin_in).unwrap_or(&f32::NAN);
                let t_arrival = *analysis.max_delay_backwards.get(&other_pin_in).unwrap_or(&f32::NAN);
                let slack = max_delay - (t_setup + t_arrival);

                if !slack.is_nan() {
                    write!(
                        input_pin_html,
                        "{}{}: {:.3} {:.3} <b>{:.3}</b><br>",
                        pin_name(&other_pin_in.0),
                        other_pin_in.1,
                        t_setup,
                        t_arrival,
                        slack
                    )
                    .unwrap();
                }
            }
        }
        writeln!(&mut html, "<td>{}</td>", input_pin_html).unwrap();

        let mut output_pin_html = String::new();

        let mut fanout_with_slack = graph.instance_fanout[instance]
            .iter()
            .flat_map(|fanout_pin_in| {
                [
                    (fanout_pin_in.clone(), Transition::Rise),
                    (fanout_pin_in.clone(), Transition::Fall),
                ]
            })
            .map(|pin| {
                let t_setup = analysis.max_delay.get(&pin).copied();
                let t_arrival = analysis.max_delay_backwards.get(&pin).copied();
                let slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
                    Some(max_delay - (t_setup + t_arrival))
                } else {
                    None
                };

                (pin, t_setup, t_arrival, slack)
            })
            .collect::<Vec<_>>();

        fanout_with_slack.sort_unstable_by_key(|(_, _, _, slack)| slack.map(|val| OrderedFloat(val)));

        for (fanout_pin_in, t_setup, t_arrival, slack) in fanout_with_slack {
            if pins_in_path.contains(&fanout_pin_in) {
                continue;
            }

            let instance = instance_name(&fanout_pin_in.0);
            let celltype = &graph.instance_celltype[&instance];
            let celltype_short = celltype.trim_start_matches("sky130_fd_sc_hd__");

            if let (Some(t_setup), Some(t_arrival), Some(slack)) = (t_setup, t_arrival, slack) {
                write!(
                    output_pin_html,
                    "{}.{}{}: {:.3} {:.3} <b>{:.3}</b><br>",
                    celltype_short,
                    pin_name(&fanout_pin_in.0),
                    fanout_pin_in.1,
                    t_setup,
                    t_arrival,
                    slack
                )
                .unwrap();
            } else {
                write!(
                    output_pin_html,
                    "{}.{}{}<br>",
                    celltype_short,
                    pin_name(&fanout_pin_in.0),
                    fanout_pin_in.1
                )
                .unwrap();
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
