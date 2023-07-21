// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file is responsible for clique computation

use massa_models::{
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
};

/// Computes max cliques of compatible blocks
pub fn compute_max_cliques(
    gi_head: &PreHashMap<BlockId, PreHashSet<BlockId>>,
) -> Vec<PreHashSet<BlockId>> {
    let mut max_cliques: Vec<PreHashSet<BlockId>> = Vec::new();

    // algorithm adapted from IK_GPX as summarized in:
    //   Cazals et al., "A note on the problem of reporting maximal cliques"
    //   Theoretical Computer Science, 2008
    //   https://doi.org/10.1016/j.tcs.2008.05.010

    // stack: r, p, x
    let mut stack: Vec<(
        PreHashSet<BlockId>,
        PreHashSet<BlockId>,
        PreHashSet<BlockId>,
    )> = vec![(
        PreHashSet::<BlockId>::default(),
        gi_head.keys().cloned().collect(),
        PreHashSet::<BlockId>::default(),
    )];
    while let Some((r, mut p, mut x)) = stack.pop() {
        if p.is_empty() && x.is_empty() {
            max_cliques.push(r);
            continue;
        }
        // choose the pivot vertex following the GPX scheme:
        // u_p = node from (p \/ x) that maximizes the cardinality of (P \ Neighbors(u_p, GI))
        let &u_p = p
            .union(&x)
            .max_by_key(|&u| {
                p.difference(&(&gi_head[u] | &vec![*u].into_iter().collect()))
                    .count()
            })
            .unwrap(); // p was checked to be non-empty before

        // iterate over u_set = (p /\ Neighbors(u_p, GI))
        let u_set: PreHashSet<BlockId> = &p & &(&gi_head[&u_p] | &vec![u_p].into_iter().collect());
        for u_i in u_set.into_iter() {
            p.remove(&u_i);
            let u_i_set: PreHashSet<BlockId> = vec![u_i].into_iter().collect();
            let comp_n_u_i: PreHashSet<BlockId> = &gi_head[&u_i] | &u_i_set;
            stack.push((&r | &u_i_set, &p - &comp_n_u_i, &x - &comp_n_u_i));
            x.insert(u_i);
        }
    }
    max_cliques
}

/// Tests

#[cfg(test)]
mod tests {
    use crate::state::clique_computation::compute_max_cliques;
    use itertools::Itertools;
    use massa_models::{
        block_id::BlockId,
        prehash::{PreHashMap, PreHashSet},
    };
    use rand::Rng;

    #[test]
    fn test_compute_max_cliques() {
        // Define the maximum size of the graph and the number of iterations
        const MAX_SIZE: usize = 10;
        const ITERATIONS: usize = 1000;

        // Generate random test cases and run the algorithm
        let mut rng = rand::thread_rng();

        for _ in 0..ITERATIONS {
            // Generate a random graph size
            let size = rng.gen_range(0..=MAX_SIZE);

            // Generate random incompatibility relationships
            let mut gi_head = PreHashMap::default();
            for i in 0..size {
                gi_head.insert(
                    BlockId::generate_from_hash(massa_hash::Hash::compute_from(&i.to_be_bytes())),
                    PreHashSet::default(),
                );
            }
            for i in 0..size.saturating_sub(1) {
                for j in (i + 1)..size {
                    // Generate a random compatibility relationship
                    let is_compatible = rng.gen_bool(0.5);

                    if !is_compatible {
                        let i_id = BlockId::generate_from_hash(massa_hash::Hash::compute_from(
                            &i.to_be_bytes(),
                        ));
                        let j_id = BlockId::generate_from_hash(massa_hash::Hash::compute_from(
                            &j.to_be_bytes(),
                        ));
                        // Add the incompatibility relationship to gi_head
                        gi_head.entry(i_id).or_default().insert(j_id);
                        gi_head.entry(j_id).or_default().insert(i_id);
                    }
                }
            }

            // Check cliques
            assert_cliques_valid(&gi_head, &compute_max_cliques(&gi_head));
        }
    }

    /// Assert that a set of cliques is valid
    fn assert_cliques_valid(
        gi_head: &PreHashMap<BlockId, PreHashSet<BlockId>>,
        max_cliques: &Vec<PreHashSet<BlockId>>,
    ) {
        // Check that there is at least one clique
        if max_cliques.is_empty() {
            panic!("max_cliques is empty");
        }

        // Check that all cliques are unique
        for (i, clique1) in max_cliques.iter().enumerate() {
            for (j, clique2) in max_cliques.iter().enumerate() {
                if i != j && clique1 == clique2 {
                    panic!("two of the cliques are identical");
                }
            }
        }

        for clique in max_cliques {
            // Check that all pairs of vertices in the clique are compatible
            for (v1, v2) in clique.iter().tuple_combinations() {
                if gi_head[&v1].contains(&v2) || gi_head[&v2].contains(&v1) {
                    panic!("incompatible vertices found within the same clique");
                }
            }

            // Check that the clique is maximal
            for v in gi_head.keys() {
                if !clique.contains(v) {
                    if clique.iter().all(|c| !gi_head[&v].contains(&c)) {
                        panic!("a clique is non-maximal");
                    }
                }
            }
        }

        // All cliques are valid, unique and maximal
    }
}
