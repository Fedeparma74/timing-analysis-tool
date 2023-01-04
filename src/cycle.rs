use petgraph::Direction::{Incoming, Outgoing};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::block::Block;
use crate::graph::{MappedCondensedGraph, MappedGraph};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn condensate_graph(
    mut original_graph: MappedGraph,
    entry_node_latency_map: &mut HashMap<u64, u32>,
    blocks: &BTreeMap<u64, Block>,
    recursive_functions: Vec<u64>,
) -> MappedCondensedGraph {
    let mut condensed_graph = original_graph.condense_cycles();

    for condensed_node in condensed_graph.get_condensed_nodes() {
        COUNTER.fetch_add(1, Ordering::Relaxed);

        // create new graph with the blocks of the condensed node, acyclic
        let mut cycle_graph = MappedGraph::new();

        // add edges to the cycle_graph
        for block in condensed_node.iter() {
            for target in block.get_targets() {
                if !condensed_node
                    .iter()
                    .filter(|node| node.leader == target)
                    .collect::<Vec<_>>()
                    .is_empty()
                {
                    let target_block = blocks.get(&target).unwrap();
                    cycle_graph.add_edge(
                        block.clone(),
                        target_block.clone(),
                        target_block.get_latency() as f32,
                    );
                }
            }
        }

        // find the entry node of the cycle

        let mut pre_cycle_blocks = condensed_node.clone();

        if condensed_graph
            .neighbors_directed(&condensed_node, Incoming)
            .to_owned()
            .len()
            > 0
        {
            pre_cycle_blocks =
                condensed_graph.neighbors_directed(&condensed_node, Incoming)[0].to_owned();
            // it is not important which block we take, we just need one
        }

        let mut entry_block = &condensed_node[0]; // to initialize the variable

        // handling case where pre_cycle_block has more than one block --> it is a condensed node
        for block in &pre_cycle_blocks {
            for inner_block in &condensed_node {
                if block.get_targets().contains(&inner_block.leader) {
                    entry_block = inner_block;
                }
            }
        }

        let outer_nodes = condensed_graph
            .neighbors_directed(&condensed_node, Outgoing)
            .to_owned();

        let mut exit_block = entry_block.clone();

        let mut false_outer_blocks = HashMap::<Block, Vec<Block>>::new(); // exit_block -> outer_blocks

        let mut normal_cycle = false;
        // handling case where there are more than one outer block
        for outer_blocks in outer_nodes {
            // handle case where outer block has more than one block --> it is a condensed node
            for outer_block in outer_blocks.clone() {
                if let Some(cycle_block) = cycle_graph
                    .get_nodes()
                    .iter()
                    .find(|node| node.get_targets().contains(&outer_block.clone().leader))
                {
                    if cycle_block.leader == entry_block.leader {
                        normal_cycle = true;
                    } else {
                        false_outer_blocks.insert(cycle_block.clone(), outer_blocks.clone());
                    }
                    break;
                }
            }
        }

        // if the entry and exit nodes are the same
        if normal_cycle {
            // if the outer block is not the normal outer block, we need to remove it
            for outer_blocks in false_outer_blocks.values() {
                condensed_graph.remove_node(outer_blocks);
            }
        } else {
            if false_outer_blocks.len() == 0 {
                //remove incoming edges to the entry node  of the cycle except for the pre_cycle_blocks
                for (source, _, _) in cycle_graph.edges_directed(entry_block, Incoming) {
                    if !pre_cycle_blocks.contains(&source) {
                        //if it is a recursive function
                        let mut add_block = entry_block.clone();
                        add_block.leader = entry_block.leader>>1;
                        cycle_graph.add_edge(
                            source.clone(),
                            add_block.clone(),
                            entry_block.get_latency() as f32,
                        );
                        cycle_graph.add_node(add_block.clone());
                    }
                }
                println!(
                    "There is no outer block for the cycle {:x}",
                    entry_block.leader
                );
            } else {
                exit_block = false_outer_blocks.keys().next().unwrap().clone();
            }
        }

        // make the cycle acyclic
        for (source, target, _) in cycle_graph.edges_directed(entry_block, Incoming) {
            cycle_graph.remove_edge(&source, &target);
        }

        let digraph = cycle_graph.to_dot_graph();
        let graph_number = COUNTER.load(Ordering::Relaxed);
        let mut dot_file = std::fs::File::create(format!("cycle_graph_{graph_number}.dot"))
            .expect("Unable to create file");
        dot_file
            .write_all(digraph.as_bytes())
            .expect("Unable to write dot file");
        
        println!("cycle_graph_{graph_number}.dot created", graph_number = graph_number);

        let entry_node_latency = entry_block.get_latency();

        match cycle_graph.reconstruct_longest_path(
            entry_block,
            &exit_block,
            entry_node_latency as f32,
        ) {
            Ok(cycle_node_latency) => {
                let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);

                if node_incoming_edges.is_empty() {
                    // if the condensed node has no incoming edges, it is the entry node
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, cycle_node_latency as u32);
                // we choose [0] as reference for the condensed node for simplicity
                } else {
                    // if the condensed node has incoming edges, we need to update the edges
                    for (source, target, _) in node_incoming_edges {
                        condensed_graph.update_edge(&source, &target, cycle_node_latency);
                    }
                    // we use the entry_node_latency_map to save the latency of the entry node if it is a condensed node
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, condensed_node[0].get_latency());
                }
            }
            Err(_) => {
                let mut condensed_cycle_graph =
                    condensate_graph(cycle_graph.clone(), entry_node_latency_map, blocks,recursive_functions.clone());

                let condensed_cycle_graph_nodes = condensed_cycle_graph.get_nodes();

                let entry_nodes = condensed_cycle_graph_nodes
                    .iter()
                    .filter(|node| {
                        condensed_cycle_graph
                            .edges_directed(node, Incoming)
                            .is_empty()
                    })
                    .collect::<Vec<_>>();

                let condensed_cycle_entry_node = entry_nodes[0].clone(); // as this is a cycle we are sure that it has only one entry node

                let entry_node_latency =
                    match entry_node_latency_map.get(&condensed_cycle_entry_node[0].leader) {
                        // if the entry node is condensed, its latency is already in the map
                        Some(latency) => *latency,
                        None => condensed_cycle_entry_node[0].get_latency(),
                    };

                let outer_nodes = condensed_graph
                    .neighbors_directed(&condensed_node, Outgoing)
                    .to_owned();

                let mut condensed_cycle_exit_node = condensed_cycle_entry_node.clone();

                let mut false_outer_nodes = HashMap::<Vec<Block>, Vec<Block>>::new(); // exit_node -> outer_blocks

                let mut normal_cycle = false;
                // handling case where there are more than one outer block
                for outer_blocks in outer_nodes {
                    // handle case where outer block has more than one block --> it is a condensed node
                    for outer_block in outer_blocks.clone() {
                        if let Some(cycle_node) =
                            condensed_cycle_graph.get_nodes().iter().find(|node| {
                                // we assume that the exit_node is not condensed
                                node[0].get_targets().contains(&outer_block.clone().leader)
                            })
                        {
                            // we assume that the exit_node is not condensed
                            if cycle_node[0].leader == condensed_cycle_entry_node[0].leader {
                                normal_cycle = true;
                            } else {
                                false_outer_nodes.insert(cycle_node.clone(), outer_blocks.clone());
                            }

                            break;
                        }
                    }
                }

                // if the entry and exit nodes are the same
                if normal_cycle {
                    // if the outer block is not the normal outer block, we need to remove it
                    for outer_blocks in false_outer_blocks.values() {
                        condensed_cycle_graph.remove_node(outer_blocks);
                    }
                } else {
                    condensed_cycle_exit_node = false_outer_nodes.keys().next().unwrap().clone();
                }

                let cycle_node_latency = condensed_cycle_graph
                    .reconstruct_longest_path(
                        &condensed_cycle_entry_node,
                        &condensed_cycle_exit_node,
                        entry_node_latency as f32,
                    )
                    .unwrap();

                let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);
                if node_incoming_edges.is_empty() {
                    // if the node has no incoming edges, it is an entry node
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, cycle_node_latency as u32);
                // we chose [0] as reference for the condensed node for simplicity
                } else {
                    for (source, target, _) in node_incoming_edges {
                        condensed_graph.update_edge(&source, &target, cycle_node_latency);
                    }
                    entry_node_latency_map
                        .insert(condensed_node[0].leader, condensed_node[0].get_latency());
                }

                let digraph = condensed_cycle_graph.to_dot_graph();
                let mut dot_file =
                    std::fs::File::create(format!("condensed_cycle_graph_{graph_number}.dot"))
                        .expect("Unable to create file");
                dot_file
                    .write_all(digraph.as_bytes())
                    .expect("Unable to write dot file");
            }
        }
    }

    condensed_graph
}
