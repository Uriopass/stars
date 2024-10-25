use std::cmp::Reverse;
use std::fs::read_to_string;

use ordered_float::OrderedFloat;
use stars::analysis::SDFGraphAnalyzed;
use stars::graph::SDFGraph;
use stars::html::extract_html_for_manual_analysis;
use stars::instance_name;
use stars::spice::{extract_spice_for_manual_analysis, SubcktData};
use stars::types::PinTrans;

fn main() {
    let path_to_parse = std::env::args_os().nth(1).expect("No argument given");

    let content = read_to_string(path_to_parse).expect("Could not read SDF file");

    let sdf = sdfparse::SDF::parse_str(&content).expect("Could not parse SDF");

    let graph = SDFGraph::new(&sdf);

    // print_graph(&graph, &mut keys);

    let subckt_data_path = std::env::var_os("SUBCKT_FILE");
    let subckt = match subckt_data_path {
        Some(path) => Some(SubcktData::new(
            &read_to_string(path).expect("Could not read SUBCKT_FILE"),
        )),
        None => {
            eprintln!("No SUBCKT_FILE specified, skipping spice extraction");
            None
        }
    };

    let analysis = SDFGraphAnalyzed::analyze(&graph);
    let mut outputs_with_delay = graph
        .outputs
        .iter()
        .filter_map(|output| Some((output, analysis.max_delay.get(output)?)))
        .collect::<Vec<_>>();

    outputs_with_delay.sort_by_key(|(_, delay)| Reverse(OrderedFloat(**delay)));

    for (i, (output, delay)) in outputs_with_delay.into_iter().skip(44).take(1).enumerate() {
        println!("{}  -- {}{}:\t{:.3}", i, output.0, output.1, delay);
        let path = analysis.extract_path(&graph, output);
        for ((pin, transition), delay) in &path {
            let instance = instance_name(pin);
            let celltype = graph.instance_celltype.get(&instance);
            println!(
                "  {} {}{:.3} {} {}",
                pin,
                transition,
                *delay,
                instance,
                celltype.unwrap_or(&String::new())
            );
        }
        let o_instance = instance_name(&output.0);
        let o_celltype = &graph.instance_celltype[&o_instance];
        println!("  {}{} {:.3} {} {}", output.0, output.1, delay, o_instance, o_celltype);

        extract_html_for_manual_analysis(&graph, &analysis, output, *delay, &path);
        if let Some(subckt) = &subckt {
            extract_spice_for_manual_analysis(&graph, &analysis, &subckt, output, *delay, &path);
        }
    }
}
#[allow(dead_code)]
fn print_graph(graph: &SDFGraph) {
    let mut keys: Vec<&PinTrans> = graph.graph.keys().collect();

    keys.sort_unstable();

    for inputs in &graph.inputs {
        println!("input: {}{}", inputs.0, inputs.1);
    }

    for outputs in &graph.outputs {
        println!("output: {}{}", outputs.0, outputs.1);
    }

    for key in keys {
        let edges = graph.graph.get(key).unwrap();
        for edge in edges {
            println!("{}{} -> {}{}\t{:.3}", key.0, key.1, edge.dst.0, edge.dst.1, edge.delay);
        }
    }
}
