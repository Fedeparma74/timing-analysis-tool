use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use petgraph::Direction::{Incoming, Outgoing};

use crate::block::Block;
use crate::graph::{MappedCondensedGraph, MappedGraph};
use crate::MAX_CYCLES;

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
                if !condensed_graph.node_index_map.contains_key(&target) {
                    let target_block = blocks.get(&target).unwrap();
                    cycle_graph.add_edge(
                        block.clone(),
                        target_block.clone(),
                        target_block.get_latency() as f32,
                    );
                }
            }
        }

        // remove incoming edge of entry node
        let cycle_entry_block = condensed_node[0].clone(); // entry node is always the first block

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

        // find the longest path in the cycle graph

        match cycle_graph.longest_path(&cycle_entry_block) {
            Ok(max_path_latency) => {
                // calculate the total latency of the cycle
                let cycle_latency = cycle_entry_block.get_latency() + max_path_latency as u32;

                // get the outer block of the cyclic node (it's always only one because it's the exit condition of the cycle)
                let outer_block =
                    condensed_graph.neighbors_directed(&condensed_node, Outgoing)[0][0].to_owned();

                // get the cycle exit block in the original graph
                let exit_block = &original_graph.neighbors_directed(&outer_block, Incoming)[0];

                let direct_path_latency =
                    cycle_latency - cycle_graph.longest_path(exit_block).unwrap() as u32;

                // println!(
                //     "Cycle latency: {}, direct path latency: {}",
                //     cycle_latency, direct_path_latency
                // );

                let cycle_node_latency = direct_path_latency + MAX_CYCLES * cycle_latency;
                // println!("Per cycle latency: {}", cycle_node_latency);

                let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);
                // println!("Incoming edges: {:?}", node_incoming_edges);
                if node_incoming_edges.is_empty() {
                    // if the node has no incoming edges, it is an entry node
                    entry_node_latency_map.insert(condensed_node[0].leader, cycle_node_latency);
                } else {
                    for (source, target, _) in node_incoming_edges {
                        condensed_graph.update_edge(&source, &target, cycle_node_latency as f32);
                    }
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, condensed_node[0].get_latency());
                }

                return condensed_graph;
            }
            Err(_) => {
                let condensed_cycle_graph =
                    condensate_graph(cycle_graph.clone(), entry_node_latency_map, blocks);

                let digraph = condensed_cycle_graph.to_dot_graph();
                let mut dot_file = std::fs::File::create(format!(
                    "condensed_cycle_graph_{}.dot",
                    COUNTER.load(Ordering::Relaxed)
                ))
                .expect("Unable to create file");
                dot_file
                    .write_all(digraph.as_bytes())
                    .expect("Unable to write dot file");

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

                let max_path_latency = condensed_cycle_graph
                    .longest_path(&condensed_cycle_entry_node)
                    .unwrap();

                println!("Max path latency: {}", max_path_latency);

                let entry_node_latency =
                    match entry_node_latency_map.get(&condensed_cycle_entry_node[0].leader) {
                        Some(latency) => *latency as u32,
                        None => condensed_cycle_entry_node[0].get_latency(),
                    };

                // calculate the total latency of the cycle
                let cycle_latency = entry_node_latency + max_path_latency as u32;

                // get the outer block of the cyclic node (it's always only one because it's the exit condition of the cycle)
                let outer_block =
                    condensed_graph.neighbors_directed(&condensed_node, Outgoing)[0][0].to_owned();

                println!("Outer block: {:?}", outer_block);

                // get the cycle exit block in the original graph
                let exit_block = &original_graph.neighbors_directed(&outer_block, Incoming)[0];

                println!("Exit block: {:?}", exit_block);

                let direct_path_latency = cycle_latency
                    - condensed_cycle_graph
                        .longest_path(&vec![exit_block.to_owned()])
                        .unwrap() as u32;

                println!(
                    "Cycle latency: {}, direct path latency: {}",
                    cycle_latency, direct_path_latency
                );

                let cycle_node_latency = direct_path_latency + MAX_CYCLES * cycle_latency;
                println!("Per cycle latency: {}", cycle_node_latency);

                let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);
                if node_incoming_edges.is_empty() {
                    // if the node has no incoming edges, it is an entry node
                    entry_node_latency_map.insert(condensed_node[0].leader, cycle_node_latency);
                } else {
                    for (source, target, _) in node_incoming_edges {
                        condensed_graph.update_edge(&source, &target, cycle_node_latency as f32);
                    }
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, condensed_node[0].get_latency());
                }

                return condensed_graph;
            }
        }
    }

    return condensed_graph;

    //     //update latency incoming node, only 1 --> condensed_cycle_graph returned
    //     let node_incoming_edges =
    //         condensed_cycle_graph.edges_directed(&condensed_cycle_node, Incoming);
    //     for (source, target, _) in node_incoming_edges {
    //         condensed_cycle_graph.update_edge(
    //             &source,
    //             &target,
    //             TOTAL_CYCLE_LATENCY.load(Ordering::Relaxed) as f32,
    //         );
    //     }

    //     //calculate latency of the cycle --> condensed_graph

    //     //update latency incoming node, only 1 --> condensed graph initially passed

    //     let max_path_latency = cycle_graph.longest_path(&cycle_entry_block).unwrap();

    //     // calculate the total latency of the cycle
    //     let cycle_latency = cycle_entry_block.get_latency() + max_path_latency as u32;

    //     // get the outer block of the cyclic node (it's always only one because it's the exit condition of the cycle)
    //     let outer_block =
    //         condensed_graph.neighbors_directed(&condensed_node, Outgoing)[0][0].to_owned();

    //     // get the cycle exit block in the original graph
    //     let exit_block = &original_graph.neighbors_directed(&outer_block, Incoming)[0];

    //     let direct_path_latency =
    //         cycle_latency - cycle_graph.longest_path(exit_block).unwrap() as u32;

    //     println!(
    //         "Cycle latency: {}, direct path latency: {}",
    //         cycle_latency, direct_path_latency
    //     );

    //     let cycle_node_latency = direct_path_latency + MAX_CYCLES * cycle_latency;
    //     println!("Per cycle latency: {}", cycle_node_latency);
    //     println!("Total cycles: {}", cycle_node_latency);

    //     let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);
    //     if node_incoming_edges.is_empty() {
    //         // if the node has no incoming edges, it is an entry node
    //         entry_node_latency_map.insert(
    //             condensed_node[0].leader,
    //             TOTAL_CYCLE_LATENCY.load(Ordering::Relaxed),
    //         );
    //     } else {
    //         for (source, target, _) in node_incoming_edges {
    //             condensed_graph.update_edge(
    //                 &source,
    //                 &target,
    //                 TOTAL_CYCLE_LATENCY.load(Ordering::Relaxed) as f32,
    //             );
    //         }
    //         entry_node_latency_map
    //             .insert(condensed_node[0].leader, condensed_node[0].get_latency());
    //     }
    // }

    // (condensed_graph, TOTAL_CYCLE_LATENCY.load(Ordering::Relaxed))
}
