use std::cmp::Reverse;
use std::fs::read_to_string;
use ordered_float::OrderedFloat;
use stars::{SDFGraph, SDFNode};

fn main() {
    let path_to_parse = std::env::args_os().nth(1).expect("No argument given");

    let content = read_to_string(path_to_parse).expect("Could not read SDF file");

    let sdf = sdfparse::SDF::parse_str(&content).expect("Could not parse SDF");

    let graph = SDFGraph::new(&sdf, true);

    // print_graph(&graph, &mut keys);

    let analysis = graph.analyze_reg2reg();

    let mut outputs_with_delay = graph.regs_d.iter().filter_map(|output| {
        Some((output, analysis.max_delay.get(output)?))
    }).collect::<Vec<_>>();

    outputs_with_delay.sort_by_key(|(_, delay)| Reverse(OrderedFloat(**delay)));

    for (output, delay) in outputs_with_delay.into_iter().take(5) {
        println!("{}:\t{:.3}", output, delay);
        let path = analysis.extract_path(&graph, output);
        let path_l = path.len();
        for (i, (node, transition, delay)) in path.into_iter().enumerate() {
            print!("{} {}{:.3}", node, transition, delay);
            if i + 1 < path_l {
                print!(" -> ");
            }
        }
        println!();
    }
}

#[allow(dead_code)]
fn print_graph(graph: &SDFGraph) {
    let mut keys: Vec<&SDFNode> = graph.graph.keys().collect();

    numeric_sort::sort_unstable(&mut keys);

    for inputs in &graph.inputs {
        println!("input: {}", inputs);
    }

    for outputs in &graph.outputs {
        println!("output: {}", outputs);
    }

    for key in keys {
        let edges = graph.graph.get(key).unwrap();
        for edge in edges {
            println!("{} -> {}\t↗{:.3} ↘{:.3}", key, edge.dst, edge.delay_pos, edge.delay_neg);
        }
    }
}
