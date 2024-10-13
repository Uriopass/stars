use std::cmp::Reverse;
use std::fs::read_to_string;
use ordered_float::OrderedFloat;
use stars::{PinMap, SDFCellType, SDFGraph, SDFGraphAnalyzed, SDFInstance, SDFPin, Transition};

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

    for (output, delay) in outputs_with_delay.into_iter().skip(1).take(1) {
        println!("{}:\t{:.3}", output, delay);
        let path = analysis.extract_path(&graph, output);
        for (pin, transition, delay) in &path {
            let instance = &graph.pin_instance[pin];
            let celltype = &graph.instance_celltype[instance];
            println!("  {} {}{:.3} {} {}", pin, transition, *delay, instance, celltype);
        }
        let o_instance = output.rsplit_once("/").unwrap().0;
        let o_celltype = &graph.instance_celltype[o_instance];
        println!("  {} {:.3} {} {}", output, delay, o_instance, o_celltype);

        let extracted = extract_path_for_manual_analysis(&graph, &analysis, output, &path);
        println!("{:#?}", extracted);
    }
}

// struct containing the path with context
// context includes: Incoming delay and outgoing delays (maybe more after)
#[derive(Debug)]
struct ExtractedPathWithContext {
    instances: Vec<(SDFInstance, SDFCellType)>,
    wires: Vec<(SDFPin, SDFPin)>,
    arrivals: PinMap<f32>,
    constraints: PinMap<f32>,
}

fn extract_path_for_manual_analysis(graph: &SDFGraph, analysis: &SDFGraphAnalyzed, output: &SDFPin, path: &[(SDFPin, Transition, f32)]) -> ExtractedPathWithContext {
    let mut instances: Vec<(SDFInstance, SDFCellType)> = vec![];
    let mut wires = vec![];
    let mut arrivals: PinMap<_> = Default::default();
    let mut constraints: PinMap<_> = Default::default();

    let mut last_pin: Option<&SDFPin> = None;
    for (pin, trans, delay) in path {
        constraints.remove(pin);
        let instance = &graph.pin_instance[pin];
        let celltype = &graph.instance_celltype[instance];
        if instances.last().map(|v| &v.0) != Some(instance) {
            instances.push((instance.clone(), celltype.clone()));
            if let Some(v) = last_pin {
                wires.push((v.clone(), pin.clone()));
            }
        } else if !last_pin.is_none() {
            for pin_in in &graph.instance_ins[instance] {
                if pin_in == pin {
                    eprintln!("weird...");
                    continue;
                }
                arrivals.insert(pin_in.clone(), *analysis.max_delay.get(pin_in).unwrap_or(&0.0));
            }
            for pin_out in &graph.instance_fanout[instance] {
                constraints.insert(pin_out.clone(), *analysis.max_delay_backwards.get(pin_out).unwrap_or(&0.0));
            }
        }

        last_pin = Some(pin);
    }
    let o_instance = output.rsplit_once("/").unwrap().0;
    let o_celltype = &graph.instance_celltype[o_instance];

    instances.push((o_instance.to_string(), o_celltype.clone()));
    wires.push((last_pin.unwrap().clone(), output.clone()));

    ExtractedPathWithContext {
        instances,
        wires,
        arrivals,
        constraints,
    }
}

#[allow(dead_code)]
fn print_graph(graph: &SDFGraph) {
    let mut keys: Vec<&SDFPin> = graph.graph.keys().collect();

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
