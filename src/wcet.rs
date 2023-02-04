use std::collections::{hash_map, BTreeMap, HashMap, HashSet};
use std::io::Write;

use capstone::{Capstone, Instructions};
use petgraph::Direction::Incoming;

use crate::arch::ArchMode;
use crate::block::Block;
use crate::cycle::condensate_graph;
use crate::graph::MappedGraph;
use crate::jump::{get_exit_jump, ExitJump};
use crate::printwarning;

pub fn calculate_wcet(cs: &Capstone, arch_mode: &ArchMode, instructions: &Instructions) -> u32 {
    let mut leaders = HashSet::new();
    let mut jumps: HashMap<u64, ExitJump> = HashMap::new(); // jump_address -> ExitJump
    let mut call_map = HashMap::<u64, u64>::new(); // call_target_address -> return_addresses (ret)
    let mut duplicated = HashMap::<(u64, u64), (u64, u64)>::new(); // (call_target_address, call_insn_address) -> (fictious address, return_address)
    let mut counter = 0;
    let mut vacant_ret = Vec::<u64>::new();

    // iteration to find all leaders and exit jumps
    instructions.windows(2).for_each(|window| {
        let instruction = &window[0];
        let next_instruction = &window[1];

        let insn_detail = cs.insn_detail(instruction).unwrap();

        let exit_jump = get_exit_jump(instruction, next_instruction, &insn_detail, arch_mode.arch);

        // if the instruction is a jump, add the jump target address and the next instruction address to the leaders
        // Then add the jump instruction to the jumps map
        if let Some(exit_jump) = exit_jump {
            if !matches!(exit_jump, ExitJump::Call(_, _)) {
                jumps.insert(instruction.address(), exit_jump.clone());
                // insert next instruction as leader
                leaders.insert(next_instruction.address());
            }

            match exit_jump {
                ExitJump::UnconditionalAbsolute(target)
                | ExitJump::UnconditionalRelative(target) => {
                    leaders.insert(target);
                }
                ExitJump::ConditionalAbsolute { taken, .. }
                | ExitJump::ConditionalRelative { taken, .. } => {
                    leaders.insert(taken);
                    // not taken is the next instruction, so it is already inserted
                }
                ExitJump::Indirect => {
                    jumps.remove(&instruction.address());
                    leaders.remove(&next_instruction.address());
                    printwarning!(
                        "Indirect jump instruction at address 0x{:x} ignored",
                        instruction.address()
                    );
                }
                ExitJump::Call(target, _) => {
                    if next_instruction.address() != target && target != instruction.address() {
                        leaders.insert(target);
                        if let hash_map::Entry::Vacant(e) = call_map.entry(target) {
                            e.insert(next_instruction.address());
                        } else {
                            let fictious_address = instruction.address() << (1 + counter);

                            if let hash_map::Entry::Vacant(e) =
                                duplicated.entry((target, instruction.address()))
                            {
                                e.insert((fictious_address, next_instruction.address()));
                                leaders.insert(fictious_address);
                            }
                            counter += 1;
                        }
                        jumps.insert(instruction.address(), exit_jump);
                        // insert next instruction as leader
                        leaders.insert(next_instruction.address());
                    } else {
                        printwarning!(
                            "External Call instruction at address 0x{:x} ignored",
                            instruction.address()
                        );
                    }
                }
                ExitJump::Ret(_) => {}
                ExitJump::Next(_) => {}
            }
        }
    });

    // iterate through all instructions and create the basic blocks
    let first_instruction = instructions.first().unwrap();
    let mut current_block: Block = Block::new(first_instruction.into());
    // we need to keep the order of the blocks to have a consistent entry point of a condensed node
    let mut blocks = BTreeMap::<u64, Block>::new();

    let mut graph = MappedGraph::new();

    // for each window of 2 instructions
    instructions
        .windows(2)
        .enumerate()
        .for_each(|(index, window)| {
            let insn = &window[0];
            let next_insn = &window[1];

            // if the next instruction is a leader, push the current block to the list of blocks
            if leaders.contains(&next_insn.address()) {
                if let Some(exit_jump) = jumps.get(&insn.address()) {
                    if call_map.contains_key(&current_block.leader) {
                        vacant_ret.push(current_block.leader);
                    }

                    if let ExitJump::Ret(_) = exit_jump {
                        if let Some(targets) = call_map.get(&current_block.leader) {
                            vacant_ret.pop().unwrap();
                            current_block.set_exit_jump(ExitJump::Ret(*targets));
                        } else if !vacant_ret.is_empty() {
                            if let Some(ret) = call_map.get(&vacant_ret.pop().unwrap()) {
                                current_block.set_exit_jump(ExitJump::Ret(*ret));
                            }
                        }
                    } else if let ExitJump::Call(target, _) = exit_jump {
                        if let Some((fictious_address, return_address)) =
                            duplicated.get(&(*target, insn.address()))
                        {
                            current_block
                                .set_exit_jump(ExitJump::Call(*fictious_address, *return_address));
                        } else {
                            current_block.set_exit_jump(exit_jump.clone());
                        }
                    } else {
                        current_block.set_exit_jump(exit_jump.clone());
                    }
                } else {
                    current_block.set_exit_jump(ExitJump::Next(next_insn.address()));
                    if call_map.contains_key(&current_block.leader) {
                        vacant_ret.push(current_block.leader);
                    }
                }

                // insert the current block to the list of blocks
                blocks.insert(current_block.leader, current_block.clone());
                current_block = Block::new(next_insn.into());
            } else {
                // push the instruction to the current block
                current_block.add_instruction(next_insn.into());
            }

            // last instruction pair -> add last instruction to block and push block (exit_jump is None)
            if index == instructions.len() - 2 {
                current_block.add_instruction(next_insn.into());
                blocks.insert(current_block.leader, current_block.clone());
            }
        });

    let mut recursive_functions = HashMap::<u64, u64>::new();
    let mut fictious_map = HashMap::<u64, u64>::new(); // real_address -> fictious address

    // add duplicated blocks to the graph for the call targets
    for ((call_target, _), (fictious_address, ret_address)) in duplicated {
        if let Some(block) = blocks.clone().get(&call_target) {
            let mut new_block = block.clone();

            if let Some(ExitJump::Ret(_)) = new_block.exit_jump {
                new_block.leader = fictious_address;
                new_block.set_exit_jump(ExitJump::Ret(ret_address));
                blocks.insert(new_block.leader, new_block.clone());
            } else {
                let mut visited_nodes = HashMap::<u64, u64>::new();

                duplicate(
                    &mut blocks,
                    &mut new_block.clone(),
                    fictious_address,
                    ret_address,
                    *call_map.get(&call_target).unwrap(),
                    &mut recursive_functions,
                    new_block.leader,
                    &mut visited_nodes,
                    &mut fictious_map,
                );
            }
        }
    }

    // add edges to the graph (it also adds the nodes)
    for block in blocks.values() {
        for target in block.get_targets() {
            if let Some(target_block) = blocks.get(&target) {
                graph.add_edge(
                    block.clone(),
                    target_block.clone(),
                    target_block.get_latency() as f32,
                );
            }
        }
    }

    let graph_dir = crate::GRAPHS_DIR;
    if !std::path::Path::new(graph_dir).exists() {
        std::fs::create_dir(graph_dir).expect("Unable to create graph directory");
    } else {
        // remove old files
        for entry in std::fs::read_dir(graph_dir).expect("Unable to read graph directory") {
            let entry = entry.expect("Unable to read graph directory");
            let path = entry.path();
            if path.is_file() {
                std::fs::remove_file(path).expect("Unable to remove file");
            }
        }
    }

    let mut dot_file =
        std::fs::File::create("graph.dot").expect("Unable to create file");
    let digraph = graph.to_dot_graph();
    dot_file
        .write_all(digraph.as_bytes())
        .expect("Unable to write dot file");

    let mut condensed_entry_node_latency = HashMap::<u64, u32>::new(); // block_leader -> latency
    let mut latency_map = HashMap::<u64, u32>::new(); // ret_address -> latency

    // condense the graph
    let condensed_graph = condensate_graph(
        graph.clone(),
        &mut condensed_entry_node_latency,
        &blocks,
        &recursive_functions,
        &mut latency_map,
        &mut fictious_map,
    );

    let mut dot_file =
        std::fs::File::create("condensed_graph.dot").expect("Unable to create file");
    let digraph = condensed_graph.to_dot_graph();
    dot_file
        .write_all(digraph.as_bytes())
        .expect("Unable to write dot file");

    // find all the entry nodes of the condesed graph
    let condensed_graph_nodes = condensed_graph.get_nodes();
    let entry_nodes = condensed_graph_nodes
        .iter()
        .filter(|node| condensed_graph.edges_directed(node, Incoming).is_empty())
        .collect::<Vec<_>>();

    let mut wcet: u32 = 0;
    let mut recursive_delay: u32 = 0;
    let mut count = 0;

    if entry_nodes.is_empty() {
        printwarning!("No entry nodes found");
    } else if entry_nodes.len() > 1 {
        for entry in entry_nodes.clone() {
            if !recursive_functions.contains_key(&entry[0].leader) {
                count += 1;
            }
        }
        if count > 1 {
            printwarning!("More than one entry node found");
        }
    }

    for entry_node in entry_nodes.clone() {
        let entry_node_latency = match condensed_entry_node_latency.get(&entry_node[0].leader) {
            Some(latency) => *latency,
            None => entry_node[0].get_latency(),
        };

        let max_path_latency = condensed_graph.longest_path(entry_node).unwrap() as u32;

        if let Some(ret_address) = recursive_functions.get(&entry_node[0].leader) {
            recursive_delay += *latency_map.get(ret_address).unwrap();
        } else {
            if count > 1 {
                println!(
                    "WCET: {} clock cycles for the graph starting at entry node: 0x{:x}",
                    entry_node_latency + max_path_latency,
                    entry_node[0].leader
                );
            }

            //calculating the wcet only if the entry node is not a recursive function
            wcet = wcet.max(entry_node_latency + max_path_latency);
        }
    }

    wcet += recursive_delay;

    wcet
}

fn duplicate(
    blocks: &mut BTreeMap<u64, Block>,
    source: &mut Block,
    fictious_address: u64,
    ret_address: u64,          // return address of the duplicated function
    original_ret_address: u64, // return address of the original function
    recursive_functions: &mut HashMap<u64, u64>, // leader -> ret_address
    call_target_address: u64,
    visited_nodes: &mut HashMap<u64, u64>, // real_address -> fictious address
    fictious_map: &mut HashMap<u64, u64>,  // fictious_address -> real_address
) {
    visited_nodes.insert(source.leader, fictious_address);
    fictious_map.insert(fictious_address, source.leader);
    let source_fictious_address = fictious_address;
    let mut fictious_address = fictious_address << (1 + 1);

    //duplicate and add to blocks all targets of the source block until a return is found
    for target in source.get_targets() {
        if let Some(target_block) = blocks.clone().get(&target) {
            //to modify one target of the source block with the new fictious address of the duplicated target block
            source.modify_targets(fictious_address, target);
            visited_nodes.insert(target, fictious_address);
            fictious_map.insert(fictious_address, target);

            let current_ret = if let Some(ExitJump::Ret(ret)) = target_block.exit_jump {
                Some(ret)
            } else {
                None
            };

            if current_ret == Some(original_ret_address) {
                let mut new_block = target_block.clone();
                new_block.leader = fictious_address;
                new_block.set_exit_jump(ExitJump::Ret(ret_address));
                blocks.insert(new_block.leader, new_block.clone());
            } else {
                let mut new_block = target_block.clone();

                if let Some(x) = target_block
                    .get_targets()
                    .iter()
                    .find(|x| visited_nodes.contains_key(x))
                {
                    if let Some(ExitJump::Call(_, ret_address)) = target_block.exit_jump {
                        if *x == call_target_address {
                            recursive_functions.insert(call_target_address, ret_address);
                        }
                    }
                    new_block.leader = fictious_address;
                    new_block.modify_targets(*visited_nodes.get(x).unwrap(), *x);
                    blocks.insert(new_block.leader, new_block.clone());
                } else {
                    duplicate(
                        blocks,
                        &mut new_block,
                        fictious_address,
                        ret_address,
                        original_ret_address,
                        recursive_functions,
                        call_target_address,
                        visited_nodes,
                        fictious_map,
                    );
                }
            }
        }

        fictious_address += 1;
    }
    source.leader = source_fictious_address;
    blocks.insert(source.leader, source.clone());
}
