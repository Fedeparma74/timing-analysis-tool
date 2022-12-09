use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use petgraph::Direction::{Incoming, Outgoing};

use crate::block::Block;
use crate::graph::{MappedCondensedGraph, MappedGraph};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn condensate_graph(
    mut original_graph: MappedGraph,
    entry_node_latency_map: &mut HashMap<u64, u32>,
    blocks: &HashMap<u64, Block>,
) -> MappedCondensedGraph {
    let mut condensed_graph = original_graph.condense_cycles();

    COUNTER.fetch_add(1, Ordering::Relaxed);

    for condensed_node in condensed_graph.get_condensed_nodes() {
        // create new graph with the blocks of the condensed node, acyclic
        let mut cycle_graph = MappedGraph::new();

        // add edges to the cycle_graph
        for block in condensed_node.iter() {
            for target in block.get_targets() {
                //if !condensed_graph.node_index_map.contains_key(&target) {
                let target_block = blocks.get(&target).unwrap();
                cycle_graph.add_edge(
                    block.clone(),
                    target_block.clone(),
                    target_block.get_latency() as f32,
                );
            }
        }

        let outer_block =
            condensed_graph.neighbors_directed(&condensed_node, Outgoing)[0].to_owned();

        cycle_graph.remove_node(&outer_block[0]);

        let exit_cycle_block = &original_graph.neighbors_directed(&outer_block[0], Incoming)[0];

        cycle_graph.remove_edge(exit_cycle_block, &outer_block[0]);

        // remove incoming edge of entry node
        //let cycle_entry_block = condensed_node[0].clone(); // entry node is always the first block //FALSE
        let outer_block =
            condensed_graph.neighbors_directed(&condensed_node, Incoming)[0].to_owned();

        //TO Do: handle case where outer block has more than one block
        if outer_block.len() > 1 {
            panic!("Outer block has more than one block");
        }

        let cycle_entry_block = &original_graph.neighbors_directed(&outer_block[0], Outgoing)[0];

        for (source, target, _) in cycle_graph.edges_directed(&cycle_entry_block, Incoming) {
            cycle_graph.remove_edge(&source, &target);
        }

        let digraph = cycle_graph.to_dot_graph();
        let mut dot_file = std::fs::File::create(format!(
            "graph_cycle_{}.dot",
            COUNTER.load(Ordering::Relaxed)
        ))
        .expect("Unable to create file");
        dot_file
            .write_all(digraph.as_bytes())
            .expect("Unable to write dot file");

        // get the outer block of the cyclic node (it's always only one because it's the exit condition of the cycle)

        let outer_block =
            condensed_graph.neighbors_directed(&condensed_node, Outgoing)[0][0].to_owned();

        // get the cycle exit block in the original graph
        let exit_block = &original_graph.neighbors_directed(&outer_block, Incoming)[0];
        let entry_node_latency = cycle_entry_block.get_latency();

        match cycle_graph.reconstruct_longest_path(
            cycle_entry_block,
            cycle_entry_block,
            exit_block,
            entry_node_latency as f32,
        ) {
            Ok(cycle_node_latency) => {
                let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);

                if node_incoming_edges.is_empty() {
                    // if the node has no incoming edges, it is an entry node
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, cycle_node_latency as u32);
                } else {
                    for (source, target, _) in node_incoming_edges {
                        condensed_graph.update_edge(&source, &target, cycle_node_latency as f32);
                    }
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, condensed_node[0].get_latency());
                }
            }
            Err(_) => {
                let mut condensed_cycle_graph =
                    condensate_graph(cycle_graph.clone(), entry_node_latency_map, blocks);

                let condensed_cycle_graph_nodes = condensed_cycle_graph.get_nodes();
                let entry_nodes = condensed_cycle_graph_nodes
                    .iter()
                    .filter(|node| {
                        condensed_cycle_graph
                            .edges_directed(node, Incoming)
                            .is_empty()
                    })
                    .collect::<Vec<_>>();

                let condensed_cycle_entry_node = entry_nodes[0].clone();

                let entry_node_latency =
                    match entry_node_latency_map.get(&condensed_cycle_entry_node[0].leader) {
                        Some(latency) => *latency as u32,
                        None => condensed_cycle_entry_node[0].get_latency(),
                    };

                // get the outer block of the cyclic node (it's always only one because it's the exit condition of the cycle)
                let outer_block =
                    condensed_graph.neighbors_directed(&condensed_node, Outgoing)[0][0].to_owned();

                // get the cycle exit block in the original graph
                let exit_block = &original_graph.neighbors_directed(&outer_block, Incoming);

                //find last block of condensated cycle graph
                let mut last_block = condensed_cycle_entry_node.clone();
                let mut last_block_found = false;
                while !last_block_found {
                    let neighbors = condensed_cycle_graph.neighbors_directed(&last_block, Outgoing);
                    if neighbors.is_empty() {
                        last_block_found = true;
                    } else {
                        last_block = neighbors[0].clone();
                    }
                }

                let cycle_node_latency = condensed_cycle_graph
                    .reconstruct_longest_path(
                        &condensed_cycle_entry_node,
                        &condensed_cycle_entry_node[0],
                        exit_block,
                        &last_block,
                        entry_node_latency as f32,
                    )
                    .unwrap();

                let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);
                if node_incoming_edges.is_empty() {
                    // if the node has no incoming edges, it is an entry node
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, cycle_node_latency as u32);
                } else {
                    for (source, target, _) in node_incoming_edges {
                        condensed_graph.update_edge(&source, &target, cycle_node_latency as f32);
                    }
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, condensed_node[0].get_latency());
                }

                let digraph = condensed_cycle_graph.to_dot_graph();
                let mut dot_file = std::fs::File::create(format!(
                    "condensed_cycle_graph_{}.dot",
                    COUNTER.load(Ordering::Relaxed)
                ))
                .expect("Unable to create file");
                dot_file
                    .write_all(digraph.as_bytes())
                    .expect("Unable to write dot file");
            }
        }
    }

    return condensed_graph;
}
