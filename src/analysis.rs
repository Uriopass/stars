use crate::graph::{SDFEdge, SDFGraph};
use crate::types::{PinTrans, PinTransMap};
use rustc_hash::FxHashSet;

pub struct SDFGraphAnalyzed {
    pub max_delay: PinTransMap<f32>,
    pub max_delay_backwards: PinTransMap<f32>,
}

impl SDFGraphAnalyzed {
    /// Extract the path of transitions that led to the max delay of the given output node.\
    /// The path is a list of (node_transition, delay) tuples, going from the start to the output.\
    ///
    /// **Note**: The output is _not_ included in the path (since it doesn't do any transitions itself).
    pub fn extract_path(&self, graph: &SDFGraph, output: &PinTrans) -> Vec<(PinTrans, f32)> {
        let mut path = Vec::new();

        let mut node = output.clone();

        loop {
            let edges = &graph.reverse_graph[&node];
            let delay = self.max_delay[&node];
            let mut prev_node_delay = None;
            for edge in edges {
                let Some(prev_delay) = self.max_delay.get(&edge.dst).copied() else {
                    continue;
                };

                if prev_delay + edge.delay == delay {
                    prev_node_delay = Some((edge.dst.clone(), prev_delay));
                }
            }
            let Some((prev_node, delay)) = prev_node_delay else {
                break;
            };
            path.push((prev_node.clone(), delay));
            node = prev_node;
        }

        path.reverse();

        path
    }
}

impl SDFGraphAnalyzed {
    /// Propagate delays through the graph and return the maximum delay for each node.
    /// The maximum delay is the maximum time it takes for a signal to propagate from the inputs to the node.
    pub fn analyze(graph: &SDFGraph) -> Self {
        fn dfs_visit<'b>(
            max_delay: &mut PinTransMap<f32>,
            node: &PinTrans,
            bw_edges_fn: impl for<'c> Fn(&'c PinTrans) -> &'b [SDFEdge] + Copy,
        ) {
            let bw_edges = bw_edges_fn(node);
            if bw_edges.is_empty() {
                max_delay.insert(node.clone(), f32::NAN);
                return;
            }

            let mut max = f32::NAN;
            for edge in bw_edges {
                let t_setup = match max_delay.get(&edge.dst) {
                    Some(delay) => *delay,
                    None => {
                        dfs_visit(max_delay, &edge.dst, bw_edges_fn);
                        max_delay[&edge.dst]
                    }
                };
                max = f32::max(max, t_setup + edge.delay);
            }

            max_delay.insert(node.clone(), max);
        }

        fn delay_pass<'b>(
            init: impl IntoIterator<Item = &'b PinTrans>,
            all_keys: impl IntoIterator<Item = &'b PinTrans>,
            bw_edges: impl for<'c> Fn(&'c PinTrans) -> &'b [SDFEdge] + Copy,
        ) -> PinTransMap<f32> {
            let init: FxHashSet<_> = init.into_iter().collect();
            let mut max_delay = PinTransMap::new();

            for &v in init.iter() {
                max_delay.insert(v.clone(), 0.0);
            }

            for v in all_keys {
                if !max_delay.contains_key(v) {
                    dfs_visit(&mut max_delay, v, bw_edges);
                }
            }

            max_delay.retain(|_, delay| !delay.is_nan());

            max_delay
        }

        let max_delay = delay_pass(graph.inputs.iter(), graph.graph.keys(), |n| &graph.reverse_graph[n]);
        let max_delay_backwards = delay_pass(graph.outputs.iter(), graph.reverse_graph.keys(), |n| &graph.graph[n]);

        Self {
            max_delay,
            max_delay_backwards,
        }
    }
}
