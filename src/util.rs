use std::collections::VecDeque;
use std::hash::Hash;
use rustc_hash::FxHashSet;

pub fn topological_sort<N, FN, IN>(mut roots: FxHashSet<N>, mut successors: FN) -> Result<Vec<N>, N>
where
    N: Eq + Hash + Clone,
    FN: FnMut(&N) -> IN,
    IN: IntoIterator<Item = N>,
{
    let mut marked = FxHashSet::with_capacity_and_hasher(roots.len(), Default::default());
    let mut temp = FxHashSet::default();
    let mut sorted = VecDeque::with_capacity(roots.len());
    while let Some(node) = roots.iter().next().cloned() {
        temp.clear();
        visit(
            &node,
            &mut successors,
            &mut roots,
            &mut marked,
            &mut temp,
            &mut sorted,
        )?;
    }
    Ok(sorted.into_iter().collect())
}

fn visit<N, FN, IN>(
    node: &N,
    successors: &mut FN,
    unmarked: &mut FxHashSet<N>,
    marked: &mut FxHashSet<N>,
    temp: &mut FxHashSet<N>,
    sorted: &mut VecDeque<N>,
) -> Result<(), N>
where
    N: Eq + Hash + Clone,
    FN: FnMut(&N) -> IN,
    IN: IntoIterator<Item = N>,
{
    unmarked.remove(node);
    if marked.contains(node) {
        return Ok(());
    }
    if temp.contains(node) {
        return Err(node.clone());
    }
    temp.insert(node.clone());
    for n in successors(node) {
        visit(&n, successors, unmarked, marked, temp, sorted)?;
    }
    marked.insert(node.clone());
    sorted.push_front(node.clone());
    Ok(())
}