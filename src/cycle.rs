use petgraph::Direction::{Incoming, Outgoing};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::block::Block;
use crate::graph::{MappedCondensedGraph, MappedGraph};
use crate::jump::ExitJump;
use crate::printwarning;

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn condensate_graph(
    mut original_graph: MappedGraph,
    entry_node_latency_map: &mut HashMap<u64, u32>,
    blocks: &BTreeMap<u64, Block>,
    recursive_functions: &HashMap<u64, u64>, // function_address -> ret_address
    latency_map: &mut HashMap<u64, u32>,     // ret_address -> latency
    fictious_map: &mut HashMap<u64, u64>,    // fictious_address -> real_address
) -> MappedCondensedGraph {
    let mut condensed_graph = original_graph.condense_cycles();

    for condensed_node in condensed_graph.get_condensed_nodes() {
        COUNTER.fetch_add(1, Ordering::Relaxed);

        // create new graph with the blocks of the condensed node, acyclic
        let mut cycle_graph = MappedGraph::new();

        // add edges to the cycle_graph
        for block in condensed_node.iter() {
            for target in block.get_targets() {
                if condensed_node.iter().any(|node| node.leader == target) {
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

        if let Some(incomings) = condensed_graph
            .neighbors_directed(&condensed_node, Incoming)
            .get(0)
        {
            pre_cycle_blocks = incomings.to_owned();
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

        let mut max_cycles = 1;
        if let Some(real_entry_address) = fictious_map.get(&entry_block.leader) {
            let env_var_key = format!("CYCLE_0x{real_entry_address:x}");
            if let Ok(cycle_var) = std::env::var(&env_var_key) {
                match cycle_var.parse::<u32>() {
                    Ok(cycle_var) => max_cycles = cycle_var,
                    Err(_) => {
                        panic!(
                            "The environment variable {} is not a valid number",
                            env_var_key
                        );
                    }
                }
            };
        } else {
            let env_var_key = format!("CYCLE_0x{:x}", entry_block.leader);
            if let Ok(cycle_var) = std::env::var(&env_var_key) {
                match cycle_var.parse::<u32>() {
                    Ok(cycle_var) => max_cycles = cycle_var,
                    Err(_) => {
                        panic!(
                            "The environment variable {} is not a valid number",
                            env_var_key
                        );
                    }
                }
            };
            printwarning!("Found a cycle at address 0x{:x} -> {max_cycles} cycle iterations considered for the wcet calculation. \
            If you want to change the value, please set the env var CYCLE_0x{:x}", entry_block.leader, entry_block.leader);
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
                printwarning!(
                    "We are not considering the exit block 0x{:x} as exit from the cycle 0x{:x}",
                    outer_blocks[0].leader,
                    entry_block.leader
                );
            }
        } else if false_outer_blocks.is_empty() {
            printwarning!(
                "There is no outer block for the cycle 0x{:x}",
                entry_block.leader
            );
        } else if false_outer_blocks.len() > 1 {
            // find the block with the highest leader and use it as exit block
            for possible_exit_block in false_outer_blocks.keys() {
                let current_leader = possible_exit_block.leader;
                if current_leader > exit_block.leader {
                    exit_block = possible_exit_block.clone();
                }
            }

            printwarning!(
                "There are more than one outer block for the cycle 0x{:x} and we are considering 0x{:x}",
                entry_block.leader, exit_block.leader
            );
        } else {
            exit_block = false_outer_blocks.keys().next().unwrap().clone();
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

        let entry_node_latency = entry_block.get_latency();

        match cycle_graph.reconstruct_longest_path(
            entry_block,
            &exit_block,
            entry_node_latency as f32,
            max_cycles,
        ) {
            Ok(cycle_node_latency) => {
                let node_incoming_edges = condensed_graph.edges_directed(&condensed_node, Incoming);

                let mut max_cycles = 1;

                // check if it is a ret
                if let Some(ExitJump::Ret(current_ret_address)) = entry_block.exit_jump {
                    for (recursive_address, ret_address) in recursive_functions {
                        if current_ret_address == *ret_address {
                            let env_var_key = format!("RECURSIVE_0x{recursive_address:x}");
                            if let Ok(recursive_var) = std::env::var(&env_var_key) {
                                match recursive_var.parse::<u32>() {
                                    Ok(recursive_var) => max_cycles = recursive_var,
                                    Err(_) => {
                                        panic!(
                                            "The environment variable {} is not a valid number",
                                            env_var_key
                                        );
                                    }
                                }
                            };
                            printwarning!(
                                "Found a recursive function at address 0x{recursive_address:x} -> {max_cycles} function iterations \
                                considered for the wcet calculation. If you want to change this value, set the environment \
                                variable {env_var_key}"
                                );
                        }
                    }
                    latency_map.insert(
                        current_ret_address,
                        cycle_node_latency as u32 * max_cycles - entry_node_latency,
                    );
                }

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
                let mut condensed_cycle_graph = condensate_graph(
                    cycle_graph.clone(),
                    entry_node_latency_map,
                    blocks,
                    recursive_functions,
                    latency_map,
                    fictious_map,
                );

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

                let mut max_cycles = 1;

                if let Some(real_entry_address) =
                    fictious_map.get(&condensed_cycle_entry_node[0].leader)
                {
                    let env_var_key = format!("CYCLE_0x{real_entry_address:x}");
                    if let Ok(cycle_var) = std::env::var(&env_var_key) {
                        match cycle_var.parse::<u32>() {
                            Ok(cycle_var) => max_cycles = cycle_var,
                            Err(_) => {
                                panic!(
                                    "The environment variable {} is not a valid number",
                                    env_var_key
                                );
                            }
                        }
                    };
                }

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
                        printwarning!(
                            "We are not considering the exit block 0x{:x} as exit from the cycle 0x{:x}",
                            outer_blocks[0].leader,
                            condensed_cycle_entry_node[0].leader
                        );
                    }
                } else if false_outer_blocks.is_empty() {
                    printwarning!(
                        "There is no outer block for the cycle 0x{:x}",
                        condensed_cycle_entry_node[0].leader
                    );
                } else if false_outer_blocks.len() > 1 {
                    // find the block with the highest leader and use it as exit block
                    for possible_exit_block in false_outer_blocks.keys() {
                        let current_leader = possible_exit_block.leader;
                        if current_leader > condensed_cycle_exit_node[0].leader {
                            condensed_cycle_exit_node[0] = possible_exit_block.clone();
                        }
                    }

                    printwarning!(
                        "There are more than one outer block for the cycle 0x{:x} and we are considering 0x{:x}",
                        condensed_cycle_entry_node[0].leader, condensed_cycle_exit_node[0].leader
                    );
                } else {
                    condensed_cycle_exit_node = false_outer_nodes.keys().next().unwrap().clone();
                }

                let cycle_node_latency = condensed_cycle_graph
                    .reconstruct_longest_path(
                        &condensed_cycle_entry_node,
                        &condensed_cycle_exit_node,
                        entry_node_latency as f32,
                        max_cycles,
                    )
                    .unwrap();

                let mut max_rec_cycles = 1;

                // check if it is a ret //condesed_cycle_entry_node[0]???
                if let Some(ExitJump::Ret(current_ret_address)) = entry_block.exit_jump { 
                    for (recursive_address, ret_address) in recursive_functions {
                        if current_ret_address == *ret_address {
                            let env_var_key = format!("RECURSIVE_0x{recursive_address:x}");
                            if let Ok(recursive_var) = std::env::var(&env_var_key) {
                                match recursive_var.parse::<u32>() {
                                    Ok(recursive_var) => max_rec_cycles = recursive_var,
                                    Err(_) => {
                                        panic!(
                                            "The environment variable {} is not a valid number",
                                            env_var_key
                                        );
                                    }
                                }
                            };
                            printwarning!(
                                "Found a recursive function at address 0x{recursive_address:x} -> {max_rec_cycles} function iterations \
                                considered for the wcet calculation. If you want to change this value, set the environment \
                                variable {env_var_key}"
                                );
                        }
                    }

                    //find the ret/next pattern of a recursive function
                    let mut ret_latency : u64 =0;
                    for node in condensed_cycle_graph.get_nodes() {
                        if let Some(ExitJump::Ret(_)) = node[0].exit_jump {
                            if node[0].leader != entry_block.leader {
                                let next_block = condensed_cycle_graph.neighbors_directed(&node, Outgoing)[0][0].clone();
                                if let Some(ExitJump::Next(_)) = next_block.exit_jump {
                                        ret_latency += node[0].get_latency() as u64;
                                        ret_latency += next_block.get_latency() as u64;
                                        println!("ret_latency: {}", ret_latency);
                                        break;
                                }
                            }
                        }
                    }
                    
                    latency_map.insert(
                        current_ret_address,
                        (cycle_node_latency as u32 - entry_node_latency) * max_rec_cycles + ret_latency as u32 * (max_rec_cycles - 1),
                    );

                    println!("latency_map: {:?}", latency_map);
                }

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
