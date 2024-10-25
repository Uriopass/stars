pub mod html;
pub mod spice;

use miniserde::Deserialize;
use rustc_hash::{FxHashMap, FxHashSet};
use sdfparse::{SDFBus, SDFDelay, SDFIOPathCond, SDFPath, SDFPort, SDFPortEdge, SDFValue};
use std::cmp::PartialEq;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct SDFEdge {
    pub dst: PinTrans,
    pub delay: f32,
}

pub type SDFPin = String;
pub type SDFInstance = String;
pub type SDFCellType = String;
pub type PinTrans = (SDFPin, Transition);
pub type PinMap<V> = BTreeMap<SDFPin, V>;
pub type PinTransMap<V> = BTreeMap<PinTrans, V>;
pub type PinSet = BTreeSet<SDFPin>;
pub type PinTransSet = BTreeSet<PinTrans>;
pub type InstanceMap<V> = BTreeMap<SDFInstance, V>;

pub struct SDFGraph {
    pub graph: PinTransMap<Vec<SDFEdge>>,
    pub reverse_graph: PinTransMap<Vec<SDFEdge>>,
    pub instance_celltype: InstanceMap<String>,
    // list of pin of input of the instance
    pub instance_ins: InstanceMap<PinSet>,
    // list of pin of output of the instance
    pub instance_outs: InstanceMap<PinSet>,
    // list of pins that are connected to the output of this instance
    pub instance_fanout: InstanceMap<PinSet>,
    pub regs_d: Vec<PinTrans>,
    pub regs_q: Vec<PinTrans>,
    pub inputs: Vec<PinTrans>,
    pub outputs: Vec<PinTrans>,
    pub clk: Option<SDFPin>,
    pub rst: Option<SDFPin>,
}

fn unique_name(path: &SDFPath) -> SDFPin {
    let mut name = String::new();
    for part in &path.path {
        name.push_str(part);
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

fn unique_name_port(cell_name: &SDFPin, port: &SDFPort) -> SDFPin {
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
        SDFValue::Multi(v, _, _) => v.unwrap_or(0.0),
    }
}

fn parse_delays(value: &[SDFValue]) -> (f32, f32) {
    match value {
        [updown] => {
            let v = extract_delay(updown);
            (v, v)
        }
        [up, down] => (extract_delay(up), extract_delay(down)),
        _ => panic!(
            "Interconnect delay is not of length 1 or 2 (up, down), but {:?}",
            value.len()
        ),
    }
}

/// Extract the name of the pin from the full path.
/// For example, `and4/A` -> `A`
pub fn pin_name_ref(pin: &SDFPin) -> &str {
    let Some(v) = pin.rsplit_once('/') else {
        return pin;
    };
    v.1
}

/// Extract the name of the pin from the full path.
/// For example, `and4/A` -> `A`
pub fn pin_name(pin: &SDFPin) -> String {
    let Some(v) = pin.rsplit_once('/') else {
        return pin.to_string();
    };
    v.1.to_string()
}

/// Extract the name of the instance from the full path.
/// For example, `and4/A` -> `and4`
pub fn instance_name(pin: &SDFPin) -> String {
    let Some(v) = pin.rsplit_once('/') else {
        return pin.to_string();
    };
    v.0.to_string()
}

/// Turns sky130_fd_sc_hd__xor2_1 into xor2
pub fn celltype_short(celltype: &str) -> &str {
    celltype
        .trim_start_matches("sky130_fd_sc_hd__")
        .rsplit_once('_')
        .unwrap()
        .0
}

#[derive(Debug, Deserialize)]
enum TriUnate {
    #[serde(rename = "positive_unate")]
    Positive,
    #[serde(rename = "negative_unate")]
    Negative,
    #[serde(rename = "non_unate")]
    Non,
}

struct UnatenessData {
    data: FxHashMap<SDFCellType, FxHashMap<SDFPin, TriUnate>>,
}

impl UnatenessData {
    pub fn new() -> Self {
        static UNATENESS_JSON: &str = include_str!("unateness.json");
        Self {
            data: miniserde::json::from_str(UNATENESS_JSON).unwrap(),
        }
    }
}

impl SDFGraph {
    pub fn new(sdf: &sdfparse::SDF) -> Self {
        let mut graph: PinTransMap<_> = Default::default();
        let mut reverse_graph: PinTransMap<_> = Default::default();
        let mut instance_celltype: InstanceMap<_> = Default::default();
        let mut instance_ins: InstanceMap<_> = Default::default();
        let mut instance_outs: InstanceMap<_> = Default::default();
        let mut instance_fanout: InstanceMap<_> = Default::default();
        let mut regs_d = vec![];
        let mut regs_q = vec![];

        let unate = UnatenessData::new();

        for cell in &sdf.cells {
            let cell_name = unique_name(cell.instance.as_ref().unwrap_or(&SDFPath {
                path: vec![],
                bus: SDFBus::None,
            }));

            instance_celltype.insert(cell_name.clone(), cell.celltype.to_string());

            for delay in &cell.delays {
                match delay {
                    SDFDelay::Interconnect(inter) => {
                        let (up, down) = parse_delays(&inter.delay);

                        let a_name = unique_name(&inter.a);
                        let b_name = unique_name(&inter.b);

                        if let Some((instance_a, _)) = a_name.rsplit_once('/') {
                            instance_fanout
                                .entry(instance_a.to_string())
                                .or_insert_with(PinSet::new)
                                .insert(b_name.clone());
                        }

                        graph
                            .entry((a_name.clone(), Transition::Rise))
                            .or_insert_with(Vec::new)
                            .push(SDFEdge {
                                dst: (b_name.clone(), Transition::Rise),
                                delay: up,
                            });
                        graph
                            .entry((a_name.clone(), Transition::Fall))
                            .or_insert_with(Vec::new)
                            .push(SDFEdge {
                                dst: (b_name.clone(), Transition::Fall),
                                delay: down,
                            });
                        graph.entry((b_name.clone(), Transition::Rise)).or_insert_with(Vec::new);
                        graph.entry((b_name.clone(), Transition::Fall)).or_insert_with(Vec::new);

                        reverse_graph
                            .entry((b_name.clone(), Transition::Rise))
                            .or_insert_with(Vec::new)
                            .push(SDFEdge {
                                dst: (a_name.clone(), Transition::Rise),
                                delay: up,
                            });
                        reverse_graph
                            .entry((a_name.clone(), Transition::Rise))
                            .or_insert_with(Vec::new);
                        reverse_graph
                            .entry((b_name.clone(), Transition::Fall))
                            .or_insert_with(Vec::new)
                            .push(SDFEdge {
                                dst: (a_name.clone(), Transition::Fall),
                                delay: down,
                            });
                        reverse_graph
                            .entry((a_name.clone(), Transition::Fall))
                            .or_insert_with(Vec::new);
                        reverse_graph
                            .entry((b_name.clone(), Transition::Rise))
                            .or_insert_with(Vec::new);
                    }
                    SDFDelay::IOPath(cond, io) => {
                        let celltype_short = celltype_short(&cell.celltype);

                        let unate_pins = unate.data.get(celltype_short).unwrap_or_else(|| {
                            panic!("No unateness data for celltype {}", celltype_short);
                        });

                        let SDFIOPathCond::None = cond else {
                            panic!("IOPathCond is not None for {:?}", cell.instance);
                        };

                        if !matches!(io.a.edge_type, SDFPortEdge::None) {
                            panic!("edge_type is not None for {:?}", cell.instance);
                        }

                        let a_name = unique_name_port(&cell_name, &io.a.port);
                        let b_name = unique_name_port(&cell_name, &io.b);

                        instance_ins
                            .entry(cell_name.clone())
                            .or_insert_with(PinSet::new)
                            .insert(a_name.clone());
                        instance_outs
                            .entry(cell_name.clone())
                            .or_insert_with(PinSet::new)
                            .insert(b_name.clone());

                        if io.a.port.port_name == "CLK" && io.b.port_name == "Q" {
                            regs_d.push((cell_name.clone() + "/D", Transition::Rise));
                            regs_d.push((cell_name.clone() + "/D", Transition::Fall));
                            regs_q.push((cell_name.clone() + "/Q", Transition::Rise));
                            regs_q.push((cell_name.clone() + "/Q", Transition::Fall));
                        }

                        let (up, down) = parse_delays(&io.delay);

                        let unate = unate_pins.get(&io.a.port.port_name.to_string()).unwrap_or_else(|| {
                            panic!(
                                "No unateness data for pin {} of celltype {}",
                                io.a.port.port_name, celltype_short
                            );
                        });

                        match unate {
                            TriUnate::Positive => {
                                graph
                                    .entry((a_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Rise),
                                        delay: up,
                                    });
                                graph
                                    .entry((a_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Fall),
                                        delay: down,
                                    });

                                reverse_graph
                                    .entry((b_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Rise),
                                        delay: up,
                                    });
                                reverse_graph
                                    .entry((b_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Fall),
                                        delay: down,
                                    });
                            }
                            TriUnate::Negative => {
                                graph
                                    .entry((a_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Fall),
                                        delay: down,
                                    });
                                graph
                                    .entry((a_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Rise),
                                        delay: up,
                                    });

                                reverse_graph
                                    .entry((b_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Fall),
                                        delay: up,
                                    });

                                reverse_graph
                                    .entry((b_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Rise),
                                        delay: down,
                                    });
                                reverse_graph
                                    .entry((a_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new);
                                reverse_graph
                                    .entry((a_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new);
                            }
                            TriUnate::Non => {
                                graph
                                    .entry((a_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Rise),
                                        delay: up,
                                    });
                                graph
                                    .entry((a_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Fall),
                                        delay: down,
                                    });
                                graph
                                    .entry((a_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Fall),
                                        delay: down,
                                    });
                                graph
                                    .entry((a_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (b_name.clone(), Transition::Rise),
                                        delay: up,
                                    });

                                reverse_graph
                                    .entry((b_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Rise),
                                        delay: up,
                                    });
                                reverse_graph
                                    .entry((b_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Fall),
                                        delay: down,
                                    });
                                reverse_graph
                                    .entry((b_name.clone(), Transition::Rise))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Fall),
                                        delay: up,
                                    });
                                reverse_graph
                                    .entry((b_name.clone(), Transition::Fall))
                                    .or_insert_with(Vec::new)
                                    .push(SDFEdge {
                                        dst: (a_name.clone(), Transition::Rise),
                                        delay: down,
                                    });
                            }
                        }

                        graph.entry((b_name.clone(), Transition::Rise)).or_insert_with(Vec::new);
                        graph.entry((b_name.clone(), Transition::Fall)).or_insert_with(Vec::new);

                        reverse_graph
                            .entry((a_name.clone(), Transition::Rise))
                            .or_insert_with(Vec::new);
                        reverse_graph
                            .entry((a_name.clone(), Transition::Fall))
                            .or_insert_with(Vec::new);
                    }
                }
            }
        }

        let mut outputs: Vec<PinTrans> = Vec::new();
        let mut inputs: Vec<PinTrans> = Vec::new();

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

        inputs.sort_unstable();
        outputs.sort_unstable();

        let mut clk = None;
        if graph.contains_key(&("clk".to_string(), Transition::Rise)) {
            clk = Some("clk".to_string());
        } else if graph.contains_key(&("clock".to_string(), Transition::Rise)) {
            clk = Some("clock".to_string());
        } else {
            eprintln!("Warning: No clock (clk) signal found");
        }

        let mut rst = None;
        if graph.contains_key(&("rst".to_string(), Transition::Rise)) {
            rst = Some("rst".to_string());
        } else if graph.contains_key(&("reset".to_string(), Transition::Rise)) {
            rst = Some("reset".to_string());
        } else if graph.contains_key(&("resetn".to_string(), Transition::Rise)) {
            rst = Some("resetn".to_string());
        } else {
            eprintln!("Warning: No reset (rst) signal found");
        }

        inputs.retain(|v| Some(&v.0) != clk.as_ref() && Some(&v.0) != rst.as_ref());
        inputs.extend(regs_q.iter().cloned());

        outputs.extend(regs_d.iter().cloned());

        SDFGraph {
            graph,
            reverse_graph,
            instance_celltype,
            instance_ins,
            instance_outs,
            instance_fanout,
            inputs,
            outputs,
            clk,
            rst,
            regs_d,
            regs_q,
        }
    }
}

pub struct SDFGraphAnalyzed {
    pub max_delay: PinTransMap<f32>,
    pub max_delay_backwards: PinTransMap<f32>,
}

impl SDFGraph {
    /// Propagate delays through the graph and return the maximum delay for each node.
    /// The maximum delay is the maximum time it takes for a signal to propagate from the inputs to the node.
    pub fn analyze(&self) -> SDFGraphAnalyzed {
        let max_delay = self.delay_pass(self.inputs.iter(), self.graph.keys(), |g, n| {
            &g.reverse_graph.get(n).unwrap_or_else(|| {
                panic!("No reverse graph entry for node {:?}", n);
            })
        });
        let max_delay_backwards = self.delay_pass(self.outputs.iter(), self.reverse_graph.keys(), |g, n| &g.graph[n]);

        SDFGraphAnalyzed {
            max_delay,
            max_delay_backwards,
        }
    }

    fn delay_pass<'b>(
        &'b self,
        init: impl IntoIterator<Item = &'b PinTrans>,
        all_keys: impl IntoIterator<Item = &'b PinTrans>,
        bw_edges: impl for<'c> Fn(&'b Self, &'c PinTrans) -> &'b [SDFEdge] + Copy,
    ) -> PinTransMap<f32> {
        let init: FxHashSet<_> = init.into_iter().collect();
        let mut max_delay = PinTransMap::new();

        for &v in init.iter() {
            max_delay.insert(v.clone(), 0.0);
        }

        for v in all_keys {
            if !max_delay.contains_key(v) {
                self.visit(&mut max_delay, v, bw_edges);
            }
        }

        max_delay.retain(|_, delay| !delay.is_nan());

        max_delay
    }

    fn visit<'b>(
        &'b self,
        max_delay: &mut PinTransMap<f32>,
        node: &PinTrans,
        bw_edges_fn: impl for<'c> Fn(&'b Self, &'c PinTrans) -> &'b [SDFEdge] + Copy,
    ) {
        let bw_edges = bw_edges_fn(self, node);
        if bw_edges.is_empty() {
            max_delay.insert(node.clone(), f32::NAN);
            return;
        }

        let mut max = f32::NAN;
        for edge in bw_edges {
            match max_delay.get(&edge.dst) {
                None => {
                    self.visit(max_delay, &edge.dst, bw_edges_fn);
                    let delay = max_delay[&edge.dst] + edge.delay;
                    max = f32::max(max, delay);
                }
                Some(delay) => {
                    let delay = delay + edge.delay;
                    max = f32::max(max, delay);
                }
            }
        }

        max_delay.insert(node.clone(), max);
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize, PartialOrd, Ord)]
pub enum Transition {
    /// 0 -> 1
    #[serde(rename = "rise")]
    Rise,
    /// 1 -> 0
    #[serde(rename = "fall")]
    Fall,
}

impl Display for Transition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Transition::Rise => write!(f, "↗"),
            Transition::Fall => write!(f, "↘"),
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
    pub fn extract_path(&self, graph: &SDFGraph, output: &PinTrans) -> Vec<(PinTrans, f32)> {
        let mut path = Vec::new();

        fn find_prev(graph: &SDFGraph, node: &PinTrans, max_delay: &PinTransMap<f32>) -> Option<(PinTrans, f32)> {
            let edges = &graph.reverse_graph[node];
            let delay = max_delay[node];
            let mut prev = None;
            for edge in edges {
                let Some(prev_delay) = max_delay.get(&edge.dst).copied() else {
                    continue;
                };

                //println!("{} -> {}\t{}, ↗{:.3} ↘{:.3} = {}", edge.dst, node, prev_delay, edge.delay_pos, edge.delay_neg, delay);
                if prev_delay + edge.delay == delay {
                    prev = Some((edge.dst.clone(), prev_delay));
                }
            }
            prev
        }

        let mut node = output.clone();

        while let Some((prev_node, delay)) = find_prev(graph, &node, &self.max_delay) {
            path.push((prev_node.clone(), delay));
            node = prev_node;
        }

        path.reverse();

        path
    }
}
