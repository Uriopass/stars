use crate::analysis::SDFGraphAnalyzed;
use crate::graph::SDFGraph;
use crate::types::{PinSet, PinTrans, SDFInstance, Transition};
use crate::{instance_name, pin_name};
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
    let mut pins_in_path: PinSet = Default::default();

    let mut last_pin: Option<&PinTrans> = None;
    for (pin_t, _delay) in path {
        let instance = instance_name(&pin_t.0);
        let last_instance = instances.last().map(|v| &v.0);

        pins_in_path.insert(pin_t.0.clone());
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
    pins_in_path.insert(output.0.clone());
    pins_in_path.insert(last_pin.unwrap().0.clone());

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
    .nogain { display: block; }
    .gain { display: none; }
</style>
<script>
document.addEventListener('DOMContentLoaded', function() {
    document.getElementById('assume-gain').addEventListener('change', function() {
        console.log(this.checked);
        if (this.checked) {
            document.querySelectorAll('.nogain').forEach(function(e) {
                e.style.display = 'none';
            });
            document.querySelectorAll('.gain').forEach(function(e) {
                e.style.display = 'block';
            });
        } else {
            document.querySelectorAll('.nogain').forEach(function(e) {
                e.style.display = 'block';
            });
            document.querySelectorAll('.gain').forEach(function(e) {
                e.style.display = 'none';
            });
        }
    });
});
</script>
<title>Path analysis</title>
</head>
<body>
    <div style="padding: 10px;display:flex;align-items: center;">
        <input type="checkbox" id="assume-gain" />
        <label for="assume-gain" style="user-select: none;">Assume 20% faster on non-critical paths</label>
    </div>
    <table>
    <tr>
        <th>Instance</th>
        <th>Setup</th>
        <th>Arr.</th>
        <th>Input Pin: Setup, Arr, <b>Slack</b></th>
        <th>Output Cells Pin (fanout)</th>
    </tr>"#,
    );

    for (instance, pin_in, pin_out) in &instances {
        let mut pin_out = pin_out;
        let pin_out_holder = (String::new(), Transition::Rise);
        if !pins_in_path.contains(&pin_out.0) {
            pin_out = &pin_out_holder;
        }

        let mut t_setup = analysis.max_delay.get(&pin_out).copied();
        let mut t_arrival = analysis.max_delay_backwards.get(&pin_out).copied();

        if instance == &instance_name(&output.0) {
            t_setup = None;
            t_arrival = None;
        }

        writeln!(&mut html, "<tr>").unwrap();
        writeln!(
            &mut html,
            "<td><center>{}<br/>{}{} → {}{}</center></td>",
            instance,
            pin_name(&pin_in.0),
            pin_in.1,
            pin_name(&pin_out.0),
            pin_out.1
        )
        .unwrap();
        let mut writecell = |v: Option<f32>| {
            if let Some(v) = v {
                writeln!(&mut html, "<td>{:.3}</td>", v).unwrap();
            } else {
                writeln!(&mut html, "<td></td>").unwrap();
            }
        };
        writecell(t_setup);
        writecell(t_arrival);

        let mut fanin_with_slack = graph.instance_ins[instance]
            .iter()
            .filter(|fanin_pin| pin_name(&fanin_pin) != "CLK")
            .flat_map(|fanin_pin| {
                [Transition::Rise, Transition::Fall]
                    .iter()
                    .map(move |transition| (fanin_pin.clone(), *transition))
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

        fanin_with_slack.sort_unstable_by_key(|(_, _, _, slack)| OrderedFloat(slack.unwrap_or(f32::INFINITY)));

        let mut input_pin_html = String::new();
        let mut input_pin_20p = String::new();

        for (other_pin_in, mut t_setup, mut t_arrival, slack) in fanin_with_slack {
            let is_critical = pin_in.0 == other_pin_in.0;

            let write_times = |html: &mut String, t_setup, t_arrival, slack| {
                if let (Some(t_setup), Some(t_arrival), Some(slack)) = (t_setup, t_arrival, slack) {
                    write!(
                        html,
                        "{}{}{}: {:.3} {:.3} <b>{:.3}</b>{}<br>",
                        is_critical.then(|| "<b>").unwrap_or(""),
                        pin_name(&other_pin_in.0),
                        other_pin_in.1,
                        t_setup,
                        t_arrival,
                        slack,
                        is_critical.then(|| "</b>").unwrap_or("")
                    )
                    .unwrap();
                } else {
                    write!(
                        html,
                        "{}{}{}{}<br>",
                        is_critical.then(|| "<b>").unwrap_or(""),
                        pin_name(&other_pin_in.0),
                        other_pin_in.1,
                        is_critical.then(|| "</b>").unwrap_or("")
                    )
                    .unwrap();
                }
            };

            write_times(&mut input_pin_html, t_setup, t_arrival, slack);
            if !is_critical {
                t_arrival = t_arrival.map(|v| v / 1.2);
                t_setup = t_setup.map(|v| v / 1.2);
            }
            let slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
                Some(max_delay - (t_setup + t_arrival))
            } else {
                None
            };
            write_times(&mut input_pin_20p, t_setup, t_arrival, slack);
        }
        writeln!(
            &mut html,
            "<td><div class='nogain'>{}</div><div class='gain'>{}</div></td>",
            input_pin_html, input_pin_20p
        )
        .unwrap();

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

        fanout_with_slack.sort_unstable_by_key(|(_, _, _, slack)| OrderedFloat(slack.unwrap_or(f32::INFINITY)));

        let mut output_pin_html = String::new();
        let mut output_pin_20p = String::new();

        for (fanout_pin_in, mut t_setup, mut t_arrival, slack) in fanout_with_slack {
            let is_critical = pins_in_path.contains(&fanout_pin_in.0);

            let write_times = |html: &mut String, t_setup, t_arrival, slack| {
                if let (Some(t_setup), Some(t_arrival), Some(slack)) = (t_setup, t_arrival, slack) {
                    write!(
                        html,
                        "{}{}{}: {:.3} {:.3} <b>{:.3}</b>{}<br>",
                        is_critical.then(|| "<b>").unwrap_or(""),
                        &fanout_pin_in.0,
                        fanout_pin_in.1,
                        t_setup,
                        t_arrival,
                        slack,
                        is_critical.then(|| "</b>").unwrap_or("")
                    )
                    .unwrap();
                } else {
                    write!(html, "{}{}<br>", pin_name(&fanout_pin_in.0), fanout_pin_in.1).unwrap();
                }
            };

            write_times(&mut output_pin_html, t_setup, t_arrival, slack);
            if !is_critical {
                t_arrival = t_arrival.map(|v| v / 1.2);
                t_setup = t_setup.map(|v| v / 1.2);
            }
            let slack = if let (Some(t_setup), Some(t_arrival)) = (t_setup, t_arrival) {
                Some(max_delay - (t_setup + t_arrival))
            } else {
                None
            };
            write_times(&mut output_pin_20p, t_setup, t_arrival, slack);
        }
        // no pain no gain
        writeln!(
            &mut html,
            "<td><div class='nogain'>{}</div><div class='gain'>{}</div></td>",
            output_pin_html, output_pin_20p
        )
        .unwrap();

        writeln!(&mut html, "</tr>").unwrap();
    }

    writeln!(&mut html, "</table>").unwrap();
    writeln!(&mut html, "</body>").unwrap();
    writeln!(&mut html, "</html>").unwrap();

    std::fs::write("path.html", html).unwrap();
}
