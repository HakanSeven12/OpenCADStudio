//! Translation-only spatial constraints between rigid objects.

use super::Vec3;

/// Solves point-coincidence relations by translating whole rigid objects.
///
/// Each relation is `(object_a, point_on_a, object_b, point_on_b)`. Fixed
/// objects keep zero translation. A component without a fixed object anchors
/// its first object, making ordered placement deterministic.
pub fn solve_rigid_point_coincidence(
    object_count: usize,
    relations: &[(usize, [f64; 3], usize, [f64; 3])],
    fixed_objects: &[usize],
    tolerance: f64,
) -> Option<Vec<[f64; 3]>> {
    let mut edges = vec![Vec::<(usize, Vec3)>::new(); object_count];
    for &(a, point_a, b, point_b) in relations {
        if a >= object_count || b >= object_count || a == b {
            return None;
        }
        let point_a = Vec3::from(point_a);
        let point_b = Vec3::from(point_b);
        if !point_a.is_finite() || !point_b.is_finite() {
            return None;
        }
        let a_to_b = point_a - point_b;
        edges[a].push((b, a_to_b));
        edges[b].push((a, -a_to_b));
    }

    let mut fixed = vec![false; object_count];
    for &index in fixed_objects {
        if index >= object_count {
            return None;
        }
        fixed[index] = true;
    }

    let tolerance = tolerance.max(0.0);
    let mut translations = vec![None::<Vec3>; object_count];
    let mut component_seen = vec![false; object_count];
    for first in 0..object_count {
        if component_seen[first] {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![first];
        component_seen[first] = true;
        while let Some(node) = stack.pop() {
            component.push(node);
            for &(next, _) in &edges[node] {
                if !component_seen[next] {
                    component_seen[next] = true;
                    stack.push(next);
                }
            }
        }

        let anchor = component
            .iter()
            .copied()
            .find(|&node| fixed[node])
            .unwrap_or(first);
        translations[anchor] = Some(Vec3::ZERO);
        let mut queue = vec![anchor];
        while let Some(node) = queue.pop() {
            let translation = translations[node]?;
            for &(next, delta) in &edges[node] {
                let candidate = translation + delta;
                if let Some(existing) = translations[next] {
                    if existing.distance(candidate) > tolerance {
                        return None;
                    }
                } else {
                    translations[next] = Some(candidate);
                    queue.push(next);
                }
            }
        }
        if component.iter().copied().any(|node| {
            fixed[node]
                && translations[node].is_some_and(|translation| translation.length() > tolerance)
        }) {
            return None;
        }
    }

    translations
        .into_iter()
        .map(|translation| translation.map(Vec3::to_array))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driven_end_propagates_through_a_rigid_chain() {
        let translations = solve_rigid_point_coincidence(
            3,
            &[
                (0, [0.0, 0.0, 0.0], 1, [2.0, 0.0, 0.0]),
                (1, [5.0, 0.0, 0.0], 2, [9.0, 0.0, 0.0]),
            ],
            &[2],
            1.0e-9,
        )
        .unwrap();

        assert_eq!(translations, vec![[6.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0; 3]]);
    }

    #[test]
    fn inconsistent_fixed_cycle_has_no_solution() {
        assert!(solve_rigid_point_coincidence(
            3,
            &[
                (0, [0.0, 0.0, 0.0], 1, [1.0, 0.0, 0.0]),
                (1, [0.0, 0.0, 0.0], 2, [1.0, 0.0, 0.0]),
                (2, [0.0, 0.0, 0.0], 0, [1.0, 0.0, 0.0]),
            ],
            &[0],
            1.0e-9,
        )
        .is_none());
    }
}
