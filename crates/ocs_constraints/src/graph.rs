//! Partitions a constraint list into independent, decoupled groups so a
//! solver can solve each separately.
//!
//! planegcs does this (`GCS.cpp`, `System::initSolution`) by building a
//! bipartite graph — one vertex per free parameter, one per constraint, an
//! edge from a constraint to each parameter it touches — and running
//! Boost.Graph's `connected_components` over it. The result is exactly the
//! partition a plain union-find over parameters gives directly: two
//! constraints end up in the same group iff they share a parameter,
//! transitively through a chain of other constraints. That's what this does
//! instead, without pulling in a graph crate for one algorithm.

use std::collections::HashMap;

use crate::constraints::Constraint;
use crate::util::ParamId;

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

/// Groups constraint indices (`0..constraints.len()`) by shared parameters.
/// A constraint with no parameters gets its own singleton group. Group
/// order, and the order of indices within each group, are unspecified.
pub fn partition_by_shared_params(constraints: &[std::rc::Rc<dyn Constraint>]) -> Vec<Vec<usize>> {
    let n = constraints.len();
    let mut uf = UnionFind::new(n);
    let mut first_seen: HashMap<ParamId, usize> = HashMap::new();

    for (i, constr) in constraints.iter().enumerate() {
        for param in constr.params() {
            match first_seen.get(&param) {
                Some(&j) => uf.union(i, j),
                None => {
                    first_seen.insert(param, i);
                }
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        groups.entry(uf.find(i)).or_default().push(i);
    }
    groups.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::point_line::Difference;
    use crate::util::ParamStore;
    use std::rc::Rc;

    fn sorted(mut groups: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
        for g in &mut groups {
            g.sort();
        }
        groups.sort();
        groups
    }

    #[test]
    fn constraints_sharing_a_param_land_in_one_group() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        let b = store.add(2.0, false);
        let c = store.add(3.0, false);
        let d1 = store.add(0.0, false);
        let d2 = store.add(0.0, false);

        let constraints: Vec<Rc<dyn Constraint>> = vec![
            Rc::new(Difference::new(a, b, d1)), // touches a, b
            Rc::new(Difference::new(b, c, d2)), // touches b, c -- links to the first via b
        ];

        let groups = sorted(partition_by_shared_params(&constraints));
        assert_eq!(groups, vec![vec![0, 1]]);
    }

    #[test]
    fn disjoint_constraints_land_in_separate_groups() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        let b = store.add(2.0, false);
        let c = store.add(3.0, false);
        let e = store.add(4.0, false);
        let d1 = store.add(0.0, false);
        let d2 = store.add(0.0, false);

        let constraints: Vec<Rc<dyn Constraint>> = vec![
            Rc::new(Difference::new(a, b, d1)),
            Rc::new(Difference::new(c, e, d2)),
        ];

        let groups = sorted(partition_by_shared_params(&constraints));
        assert_eq!(groups, vec![vec![0], vec![1]]);
    }

    #[test]
    fn a_chain_of_shared_params_transitively_joins_three_constraints() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        let b = store.add(2.0, false);
        let c = store.add(3.0, false);
        let d = store.add(4.0, false);
        let d1 = store.add(0.0, false);
        let d2 = store.add(0.0, false);
        let d3 = store.add(0.0, false);

        let constraints: Vec<Rc<dyn Constraint>> = vec![
            Rc::new(Difference::new(a, b, d1)), // a, b
            Rc::new(Difference::new(c, d, d2)), // c, d -- separate so far
            Rc::new(Difference::new(b, c, d3)), // b, c -- bridges the two groups above
        ];

        let groups = sorted(partition_by_shared_params(&constraints));
        assert_eq!(groups, vec![vec![0, 1, 2]]);
    }
}
