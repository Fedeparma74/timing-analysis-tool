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

    let mut condensed_nodes = condensed_graph.get_condensed_nodes();

    //adding self cycles nodes
    for node in condensed_graph.get_nodes() {
        if node[0].get_targets().contains(&node[0].leader) {
            condensed_nodes.push(node.clone());
        }
    }

    for condensed_node in condensed_nodes {
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

        let incomings_vec = condensed_graph.neighbors_directed(&condensed_node, Incoming);

        let mut entry_blocks = HashMap::new();

        // handling case where pre_cycle_block has more than one block --> it is a condensed node
        for pre_cycle_blocks in &incomings_vec {
            for block in pre_cycle_blocks {
                for inner_block in &condensed_node {
                    if block.get_targets().contains(&inner_block.leader) {
                        entry_blocks.insert(inner_block.leader, inner_block.clone());
                    }
                }
            }
        }

        let outer_nodes = condensed_graph
            .neighbors_directed(&condensed_node, Outgoing)
            .to_owned();

        let mut entry_block = condensed_node[0].clone(); // to initialize the variable

        let mut false_outer_blocks = HashMap::<Block, Vec<Block>>::new(); // exit_block -> outer_blocks

        // to handle case where there is no entry block
        if entry_blocks.is_empty() {
            entry_blocks.insert(entry_block.leader, entry_block.clone());
        }

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
                    let mut remove_flag = false;
                    for (leader, entry) in entry_blocks.clone() {
                        if cycle_block.leader == leader {
                            normal_cycle = true;
                            remove_flag = true;
                            entry_block = entry.clone();
                            entry_blocks.remove(&leader);
                            break;
                        }
                    }
                    if !remove_flag {
                        false_outer_blocks.insert(cycle_block.clone(), outer_blocks.clone());
                    }
                }
            }
        }

        let mut exit_block = entry_block.clone(); // to initialize the variable

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

            for entry_leader in entry_blocks.keys() {
                printwarning!(
                    "We are not considering the entry block 0x{:x} as entry from the cycle 0x{:x}",
                    entry_leader,
                    entry_block.leader
                );
                // remove the incomings edges of the entry blocks not considered in the WCET calculation
                for pre_cycle_blocks in &incomings_vec {
                    for block in pre_cycle_blocks {
                        if block.get_targets().contains(entry_leader) {
                            condensed_graph.remove_edge(pre_cycle_blocks, &condensed_node);
                        }
                    }
                }
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

        if !normal_cycle && entry_blocks.clone().len() > 1 {
            printwarning!(
                "There are more than one entry block for the cycle 0x{:x} and none of them is also an exit block!!",
                entry_block.leader
            );
            printwarning!(
                "We thus consider the entry block 0x{:x} and the exit block 0x{:x} for this cycle",
                entry_block.leader,
                exit_block.leader
            );
        }

        // remove the oouter blocks from the condensed graph

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

        // make the cycle acyclic
        for (source, target, _) in cycle_graph.edges_directed(&entry_block, Incoming) {
            cycle_graph.remove_edge(&source, &target);
        }

        let graph_dir = crate::GRAPHS_DIR;

        let digraph = cycle_graph.to_dot_graph();
        let graph_number = COUNTER.load(Ordering::Relaxed);
        let mut dot_file =
            std::fs::File::create(format!("{graph_dir}/cycle_graph_{graph_number}.dot"))
                .expect("Unable to create file");
        dot_file
            .write_all(digraph.as_bytes())
            .expect("Unable to write dot file");

        let entry_node_latency = entry_block.get_latency();

        match cycle_graph.reconstruct_longest_path(
            &entry_block,
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
                                            "The environment variable {env_var_key} is not a valid number"
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
                        (cycle_node_latency as u32 - entry_node_latency) * max_cycles,
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

                let mut condensed_cycle_entry_node = condensed_node.clone();

                let mut entry_nodes = condensed_cycle_graph_nodes
                    .iter()
                    .filter(|node| {
                        condensed_cycle_graph
                            .edges_directed(node, Incoming)
                            .is_empty()
                            && condensed_graph
                                .get_nodes()
                                .iter()
                                .any(|n| n[0].get_targets().contains(&node[0].leader))
                    })
                    .collect::<Vec<_>>();

                if !entry_nodes.is_empty() {
                    condensed_cycle_entry_node = entry_nodes[0].clone();
                }

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
                                    "The environment variable {env_var_key} is not a valid number"
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
                            for entry in entry_nodes.clone() {
                                if entry[0].leader == cycle_node[0].leader {
                                    normal_cycle = true;
                                    condensed_cycle_entry_node = entry.clone();
                                    entry_nodes.retain(|x| x[0].leader != entry[0].clone().leader);
                                    break;
                                }
                            }
                            if !normal_cycle {
                                false_outer_nodes.insert(cycle_node.clone(), outer_blocks.clone());
                            }
                        }
                    }
                }

                let mut condensed_cycle_exit_node = condensed_cycle_entry_node.clone();

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

                    for entry in entry_nodes.clone() {
                        printwarning!(
                            "We are not considering the entry block 0x{:x} as entry from the cycle 0x{:x}",
                            entry[0].leader,
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

                if !normal_cycle && entry_nodes.len() > 1 {
                    printwarning!(
                        "There are more than one entry block for the cycle 0x{:x} and none of them is also an exit block!!",
                        condensed_cycle_entry_node[0].leader
                    );
                    printwarning!(
                        "We thus consider the entry block 0x{:x} and the exit block 0x{:x} for this cycle",
                        condensed_cycle_entry_node[0].leader,
                        condensed_cycle_exit_node[0].leader
                    );
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

                // check if it is a ret cycle
                if let Some(ExitJump::Ret(current_ret_address)) = entry_block.exit_jump {
                    for (recursive_address, ret_address) in recursive_functions {
                        if current_ret_address == *ret_address {
                            let env_var_key = format!("RECURSIVE_0x{recursive_address:x}");
                            if let Ok(recursive_var) = std::env::var(&env_var_key) {
                                match recursive_var.parse::<u32>() {
                                    Ok(recursive_var) => max_rec_cycles = recursive_var,
                                    Err(_) => {
                                        panic!(
                                            "The environment variable {env_var_key} is not a valid number"
                                        );
                                    }
                                }
                            };
                            printwarning!(
                                "Found a recursive function with multiple recursion at address 0x{recursive_address:x} -> {max_rec_cycles} function iterations \
                                considered for the wcet calculation for every recursion (PESSIMISTIC APPROACH). If you want to change this value, set the environment \
                                variable {env_var_key}"
                                );
                        }
                    }

                    if max_rec_cycles > 0 {
                        //find the return cycle pattern of a inner recursive invocation
                        let mut ret_latency: u64 = 0;
                        for node in condensed_cycle_graph.get_nodes() {
                            if let Some(ExitJump::Ret(_)) = node[0].exit_jump {
                                if node[0].leader != entry_block.leader {
                                    let next_block = condensed_cycle_graph
                                        .neighbors_directed(&node, Outgoing)[0][0]
                                        .clone();
                                    ret_latency += node[0].get_latency() as u64;
                                    ret_latency += next_block.get_latency() as u64;
                                    break;
                                }
                            }
                        }

                        latency_map.insert(
                            current_ret_address,
                            (cycle_node_latency as u32
                                - entry_node_latency as u32
                                - ret_latency as u32
                                + ret_latency as u32 * max_rec_cycles)
                                * max_rec_cycles,
                        );
                    } else {
                        latency_map.insert(
                            current_ret_address,
                            (cycle_node_latency as u32 - entry_node_latency) * max_rec_cycles,
                        );
                    }
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
                let mut dot_file = std::fs::File::create(format!(
                    "{graph_dir}/condensed_cycle_graph_{graph_number}.dot"
                ))
                .expect("Unable to create file");
                dot_file
                    .write_all(digraph.as_bytes())
                    .expect("Unable to write dot file");
            }
        }
    }

    condensed_graph
}
