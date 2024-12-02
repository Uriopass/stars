use crate::types::{
    InstanceMap, PinSet, PinTrans, PinTransMap, SDFCellType, SDFInstance, SDFPin, Transition, TriUnate,
};
use rustc_hash::FxHashMap;
use sdfparse::{SDFBus, SDFDelay, SDFIOPathCond, SDFPath, SDFPort, SDFPortEdge, SDFValue};

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct SDFEdge {
    pub dst: PinTrans,
    pub delay: f32,
}

pub struct SDFGraph {
    pub graph: PinTransMap<Vec<SDFEdge>>,
    pub reverse_graph: PinTransMap<Vec<SDFEdge>>,
    pub instance_celltype: InstanceMap<String>,
    /// list of pin of input of the instance (e.g A)
    pub instance_ins: InstanceMap<PinSet>,
    /// list of pin of output of the instance (e.g X). Most often there is only one pin in this set.
    pub instance_outs: InstanceMap<PinSet>,
    /// list of (input) pins that are connected to the output of this instance
    pub instance_fanout: InstanceMap<PinSet>,
    pub inputs: Vec<PinTrans>,
    pub outputs: Vec<PinTrans>,
}

struct UnatenessData {
    /// celltype -> pin -> unateness
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

fn extract_delay(value: &SDFValue) -> f32 {
    match *value {
        SDFValue::None => 0.0,
        SDFValue::Single(v) => v,
        SDFValue::Multi(v, _, _) => v.unwrap_or(0.0),
    }
}

fn unique_name(path: &SDFPath, renaming: &FxHashMap<String, String>) -> SDFPin {
    let mut name = String::new();
    for part in &path.path {
        if let Some(v) = renaming.get(part.as_str()) {
            name.push_str(&v);
        } else {
            name.push_str(part);
        }
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

static DO_RENAMING: bool = false;

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
        let mut renaming_map: FxHashMap<SDFInstance, String> = Default::default();

        let unate = UnatenessData::new();

        if DO_RENAMING {
            let mut renaming_counter: FxHashMap<SDFInstance, usize> = Default::default();
            for cell in &sdf.cells {
                let old_cell_name = unique_name(
                    cell.instance.as_ref().unwrap_or(&SDFPath {
                        path: vec![],
                        bus: SDFBus::None,
                    }),
                    &FxHashMap::default(),
                );
                let celltype_short = crate::celltype_short_with_size(&cell.celltype);
                let rename_i = renaming_counter.entry(celltype_short.to_string()).or_insert(0);
                *rename_i += 1;
                let cell_name = format!("{rename_i:03}_{celltype_short}");
                renaming_map.insert(old_cell_name, cell_name);
            }
        }

        for cell in &sdf.cells {
            let cell_name = unique_name(
                cell.instance.as_ref().unwrap_or(&SDFPath {
                    path: vec![],
                    bus: SDFBus::None,
                }),
                &renaming_map,
            );
            instance_celltype.insert(cell_name.clone(), cell.celltype.to_string());

            for delay in &cell.delays {
                match delay {
                    SDFDelay::Interconnect(inter) => {
                        let (up, down) = parse_delays(&inter.delay);

                        let a_name = unique_name(&inter.a, &renaming_map);
                        let b_name = unique_name(&inter.b, &renaming_map);

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
                        let celltype_short = crate::celltype_short(&cell.celltype);
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
        }
    }
}
