use std::fmt::{Display, Formatter};
use ordered_float::OrderedFloat;
use rustc_hash::{FxHashMap, FxHashSet};
use sdfparse::{SDFBus, SDFDelay, SDFIOPathCond, SDFPath, SDFPort, SDFPortEdge, SDFValue};

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct SDFEdge {
    pub dst: SDFNode,
    pub delay_pos: f32,
    pub delay_neg: f32,
}

pub type SDFNode = String;

pub struct SDFGraph {
    pub graph: FxHashMap<SDFNode, Vec<SDFEdge>>,
    pub reverse_graph: FxHashMap<SDFNode, Vec<SDFEdge>>,
    pub regs_d: Vec<SDFNode>,
    pub regs_q: Vec<SDFNode>,
    pub inputs: Vec<SDFNode>,
    pub outputs: Vec<SDFNode>,
    pub clk: Option<SDFNode>,
    pub rst: Option<SDFNode>,
}

fn unique_name(path: &SDFPath) -> SDFNode {
    let mut name = String::new();
    for part in &path.path {
        name.push_str(&part);
        name.push('/');
    }
    name.pop();
    match path.bus {
        SDFBus::None => {}
        SDFBus::SingleBit(b) => {
            name.push('[');
            name.push_str(&b.to_string());
            name.push(']');
        }
        SDFBus::BitRange(_, _) => {
            unimplemented!("SDFBus::BitRange");
        }
    }
    name
}

fn unique_name_port(cell_name: &SDFNode, port: &SDFPort) -> SDFNode {
    let mut name = cell_name.clone();
    name.push('/');
    name.push_str(&port.port_name);
    match port.bus {
        SDFBus::None => {}
        SDFBus::SingleBit(b) => {
            name.push('[');
            name.push_str(&b.to_string());
            name.push(']');
        }
        SDFBus::BitRange(_, _) => {
            unimplemented!("SDFBus::BitRange");
        }
    }
    name
}

fn extract_delay(value: &SDFValue) -> f32 {
    match *value {
        SDFValue::None => 0.0,
        SDFValue::Single(v) => v,
        SDFValue::Multi(v, _, _) => {
            v.unwrap_or(0.0)
        }
    }
}

fn parse_delays(value: &[SDFValue]) -> (f32, f32) {
    match value {
        [updown] => {
            let v = extract_delay(updown);
            (v, v)
        },
        [up, down] => (extract_delay(up), extract_delay(down)),
        _ => panic!("Interconnect delay is not of length 1 or 2 (up, down), but {:?}", value.len()),
    }
}

impl SDFGraph {
    pub fn new(sdf: &sdfparse::SDF, check_cycle: bool) -> Self {
        let mut graph: FxHashMap<_, _> = FxHashMap::with_capacity_and_hasher(sdf.cells.len(), Default::default());
        let mut reverse_graph: FxHashMap<_, _> = FxHashMap::with_capacity_and_hasher(sdf.cells.len(), Default::default());
        let mut regs_d = vec![];
        let mut regs_q = vec![];

        for cell in &sdf.cells {
            let cell_name = unique_name(cell.instance.as_ref().unwrap_or(&SDFPath {
                path: vec![],
                bus: SDFBus::None,
            }));

            for delay in &cell.delays {
                match delay {
                    SDFDelay::Interconnect(inter) => {
                        let (up, down) = parse_delays(&inter.delay);

                        let a_name = unique_name(&inter.a);
                        let b_name = unique_name(&inter.b);

                        graph.entry(a_name.clone()).or_insert_with(Vec::new).push(SDFEdge {
                            dst: b_name.clone(),
                            delay_pos: up,
                            delay_neg: down,
                        });
                        graph.entry(b_name.clone()).or_insert_with(Vec::new);

                        reverse_graph.entry(b_name).or_insert_with(Vec::new).push(SDFEdge {
                            dst: a_name.clone(),
                            delay_pos: up,
                            delay_neg: down,
                        });
                        reverse_graph.entry(a_name).or_insert_with(Vec::new);
                    }
                    SDFDelay::IOPath(cond, io) => {
                        let SDFIOPathCond::None = cond else {
                            panic!("IOPathCond is not None for {:?}", cell.instance);
                        };

                        if !matches!(io.a.edge_type, SDFPortEdge::None) {
                            panic!("edge_type is not None for {:?}", cell.instance);
                        }

                        let a_name = unique_name_port(&cell_name, &io.a.port);
                        let b_name = unique_name_port(&cell_name, &io.b);

                        if io.a.port.port_name == "CLK" && io.b.port_name == "Q" {
                            regs_d.push(cell_name.clone() + "/D");
                            regs_q.push(cell_name.clone() + "/Q");
                        }

                        let (up, down) = parse_delays(&io.delay);

                        graph.entry(a_name.clone()).or_insert_with(Vec::new).push(SDFEdge {
                            dst: b_name.clone(),
                            delay_pos: up,
                            delay_neg: down,
                        });
                        graph.entry(b_name.clone()).or_insert_with(Vec::new);

                        reverse_graph.entry(b_name).or_insert_with(Vec::new).push(SDFEdge {
                            dst: a_name.clone(),
                            delay_pos: up,
                            delay_neg: down,
                        });
                        reverse_graph.entry(a_name).or_insert_with(Vec::new);
                    }
                }
            }
        }

        let mut outputs: Vec<SDFNode> = Vec::new();
        let mut inputs: Vec<SDFNode> = Vec::new();

        for (key, edges) in &graph {
            if edges.is_empty() {
                outputs.push(key.clone());
            }
        }

        for (key, edges) in &reverse_graph {
            if edges.is_empty() {
                inputs.push(key.clone());
            }
        }

        numeric_sort::sort_unstable(&mut inputs);
        numeric_sort::sort_unstable(&mut outputs);

        if check_cycle {
            if Self::has_cycle(&graph, &inputs) {
                panic!("graph has cycle :(");
            }
        }

        let mut clk = None;
        if graph.contains_key("clk") {
            clk = Some("clk".to_string());
        } else if graph.contains_key("clock") {
            clk = Some("clock".to_string());
        } else {
            eprintln!("Warning: No clock (clk) signal found");
        }

        let mut rst = None;
        if graph.contains_key("rst") {
            rst = Some("rst".to_string());
        } else if graph.contains_key("reset") {
            rst = Some("reset".to_string());
        } else if graph.contains_key("resetn") {
            rst = Some("resetn".to_string());
        } else {
            eprintln!("Warning: No reset (rst) signal found");
        }

        SDFGraph {
            graph,
            reverse_graph,
            inputs,
            outputs,
            clk,
            rst,
            regs_d,
            regs_q,
        }
    }

    fn has_cycle_dfs(graph: &FxHashMap<SDFNode, Vec<SDFEdge>>, node: &SDFNode, visited: &mut FxHashSet<SDFNode>, stack: &mut FxHashSet<SDFNode>) -> bool {
        if stack.contains(node) {
            return true;
        }

        if visited.contains(node) {
            return false;
        }

        visited.insert(node.clone());
        stack.insert(node.clone());

        for edge in &graph[node] {
            if Self::has_cycle_dfs(graph, &edge.dst, visited, stack) {
                return true;
            }
        }

        stack.remove(node);
        false
    }

    pub fn has_cycle(graph: &FxHashMap<SDFNode, Vec<SDFEdge>>, inputs: &[SDFNode]) -> bool {
        let mut visited = FxHashSet::default();
        let mut stack = FxHashSet::default();

        for node in inputs {
            if Self::has_cycle_dfs(graph, node, &mut visited, &mut stack) {
                return true;
            }
        }

        false
    }
}

pub struct SDFGraphAnalyzed {
    pub max_delay: FxHashMap<SDFNode, f32>,
}

impl SDFGraph {
    /// Propagate delays through the graph and return the maximum delay for each node.
    /// The maximum delay is the maximum time it takes for a signal to propagate from the inputs to the node.
    pub fn analyze_reg2reg(&self) -> SDFGraphAnalyzed {
        let mut max_delay = FxHashMap::default();

        let mut queue = priority_queue::PriorityQueue::with_capacity(self.graph.len());
        let mut visited: FxHashSet<_> = Default::default();

        for node in &self.regs_q {
            queue.push(node.clone(), OrderedFloat(0.0));
        }

        while let Some((node, OrderedFloat(delay))) = queue.pop() {
            max_delay.insert(node.clone(), delay);
            visited.insert(node.clone());

            let edges = &self.graph[&node];
            for edge in edges {
                let delay = delay + f32::max(edge.delay_pos, edge.delay_neg);

                if !visited.contains(&edge.dst) {
                    queue.push(edge.dst.clone(), OrderedFloat(delay));
                }
            }
        }

        SDFGraphAnalyzed {
            max_delay,
        }
    }
}

#[derive(Debug)]
pub enum Transition {
    /// Positive transition. 0 -> 1
    Pos,
    /// Negative transition. 1 -> 0
    Neg,
}

impl Display for Transition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Transition::Pos => write!(f, "↗"),
            Transition::Neg => write!(f, "↘"),
        }
    }
}

impl SDFGraphAnalyzed {
    /// Extract the path of transitions that led to the max delay of the given output node.\
    /// The path is a list of (node, transition, delay) tuples, going from the start to the output.\
    ///
    /// **Example**: `[(1, Pos, 0.1), (2, Neg, 0.2)]` means that the transition from 1 to 2 was a positive transition with a delay of 0.1, and the transition from 2 to the output was a negative transition with a delay of 0.2.
    ///
    /// **Note**: The output is _not_ included in the path (since it doesn't do any transitions itself).
    pub fn extract_path(&self, graph: &SDFGraph, output: &SDFNode) -> Vec<(SDFNode, Transition, f32)> {
        let mut path = Vec::new();

        fn find_prev(graph: &SDFGraph, node: &SDFNode, max_delay: &FxHashMap<SDFNode, f32>) -> Option<(SDFNode, Transition, f32)> {
            let edges = &graph.reverse_graph[node];
            let delay = max_delay[node];
            let mut prev = None;
            for edge in edges {
                let Some(prev_delay) = max_delay.get(&edge.dst).copied() else {
                    continue;
                };

                //println!("{} -> {}\t{}, ↗{:.3} ↘{:.3} = {}", edge.dst, node, prev_delay, edge.delay_pos, edge.delay_neg, delay);

                if prev_delay + edge.delay_pos == delay {
                    prev = Some((edge.dst.clone(), Transition::Pos, prev_delay));
                } else if prev_delay + edge.delay_neg == delay {
                    prev = Some((edge.dst.clone(), Transition::Neg, prev_delay));
                }
            }
            prev
        }

        let mut node = output.clone();

        while let Some((prev_node, transition, delay)) = find_prev(graph, &node, &self.max_delay) {
            path.push((prev_node.clone(), transition, delay));
            node = prev_node;
        }

        path.reverse();

        path
    }
}