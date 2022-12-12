mod arch;
mod block;
mod cycle;
mod graph;
mod instruction;
mod jump;

use std::cell::RefCell;
use std::collections::{hash_map, HashMap, HashSet};
use std::io::Write;

use capstone::{Capstone, NO_EXTRA_MODE};
use jump::get_exit_jump;
use object::{Object, ObjectSection};
use petgraph::Direction::Incoming;

use crate::arch::ArchMode;
use crate::block::Block;
use crate::cycle::condensate_graph;
use crate::graph::MappedGraph;
use crate::jump::ExitJump;

thread_local! {
    static CURRENT_ARCH: RefCell<Option<ArchMode>> = RefCell::new(None);
}

const MAX_CYCLES: u32 = 1;

fn main() {
    dotenv::dotenv().ok(); // load .env file

    let file_bytes = std::fs::read("ooribile.o").unwrap(); //prova_3ret.o --> 219, prova_d --> 229,  prova_without_cycles.o --> 139, 3cicli.o --> 241, prova_2for --> 159
    let obj_file = object::File::parse(file_bytes.as_slice()).unwrap();

    let arch = obj_file.architecture();
    let arch_mode = ArchMode::from(arch);
    CURRENT_ARCH.with(|current_arch| {
        *current_arch.borrow_mut() = Some(arch_mode.clone());
    });

    println!("{:?}", arch_mode);

    let mut text_section = Vec::new();
    for section in obj_file.sections() {
        // join all the sections .text in one
        if section.name().unwrap().contains("text") {
            text_section.extend_from_slice(section.data().unwrap());
        }
    }

    let mut cs = Capstone::new_raw(arch_mode.arch, arch_mode.mode, NO_EXTRA_MODE, None)
        .expect("Failed to create Capstone handle");
    cs.set_detail(true).unwrap();

    let instructions = cs
        .disasm_all(&text_section, 0x1000)
        .expect("Failed to disassemble given code");

    //print instructions to file
    let mut file = std::fs::File::create("instructions.txt").unwrap();
    instructions.iter().for_each(|instruction| {
        writeln!(
            file,
            "{:x}\t{:?}",
            instruction.address(),
            instruction.mnemonic()
        )
        .unwrap();
    });

    let mut leaders = HashSet::new();
    let mut jumps: HashMap<u64, ExitJump> = HashMap::new(); // jump_address -> ExitJump
    let mut call_map = HashMap::<u64, Vec<u64>>::new(); // call_target_address -> return_addresses (ret)

    // iteration to find all leaders and exit jumps
    instructions.windows(2).for_each(|window| {
        let instruction = &window[0];
        let next_instruction = &window[1];

        let insn_detail = cs.insn_detail(instruction).unwrap();

        let exit_jump = get_exit_jump(instruction, next_instruction, &insn_detail, arch_mode.arch);

        // if the instruction is a jump, add the jump target address and the next instruction address to the leaders
        // Then add the jump instruction to the jumps map
        if let Some(exit_jump) = exit_jump {
            jumps.insert(instruction.address(), exit_jump.clone());

            // insert next instruction as leader
            leaders.insert(next_instruction.address());

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
                }
                ExitJump::Call(target, _) => {
                    if next_instruction.address() != target && target != instruction.address() {
                        leaders.insert(target);
                        if let hash_map::Entry::Vacant(e) = call_map.entry(target) {
                            e.insert(vec![next_instruction.address()]);
                        } else {
                            call_map
                                .get_mut(&target)
                                .unwrap()
                                .push(next_instruction.address());
                        }
                    } else {
                        leaders.remove(&next_instruction.address());
                        jumps.remove(&instruction.address());
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
    let mut blocks = HashMap::<u64, Block>::new();
    // we need to keep the order of the blocks to have a consistent entry point of a condensed node (HashMap is not ordered)
    let mut ordered_block_leaders = Vec::<u64>::new();

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
                    if let ExitJump::Ret(_) = exit_jump {
                        if call_map.contains_key(&current_block.leader) {
                            current_block.set_exit_jump(ExitJump::Ret(
                                if let Some(targets) = call_map.get(&current_block.leader) {
                                    targets.clone()
                                } else {
                                    vec![]
                                },
                            ));
                        }
                    } else {
                        current_block.set_exit_jump(exit_jump.clone());
                    }
                } else {
                    current_block.set_exit_jump(ExitJump::Next(next_insn.address()));
                }

                // insert the current block to the list of blocks
                blocks.insert(current_block.leader, current_block.clone());
                ordered_block_leaders.push(current_block.leader);
                current_block = Block::new(next_insn.into());
            } else {
                // push the instruction to the current block
                current_block.add_instruction(next_insn.into());
            }

            // last instruction pair -> add last instruction to block and push block (exit_jump is None)
            if index == instructions.len() - 2 {
                current_block.add_instruction(next_insn.into());
                blocks.insert(current_block.leader, current_block.clone());
                ordered_block_leaders.push(current_block.leader);
            }
        });

    // add edges to the graph (it also adds the nodes)
    for block_leader in &ordered_block_leaders {
        let source_block = blocks.get(block_leader).unwrap();
        for target in source_block.get_targets() {
            let target_block = blocks.get(&target).unwrap();
            graph.add_edge(
                source_block.clone(),
                target_block.clone(),
                target_block.get_latency() as f32,
            );
        }
    }

    let mut dot_file = std::fs::File::create("graph.dot").expect("Unable to create file");

    let digraph = graph.to_dot_graph();

    dot_file
        .write_all(digraph.as_bytes())
        .expect("Unable to write dot file");

    // let mut condensed_graph = graph.condense_cycles();
    let mut condensed_entry_node_latency = HashMap::<u64, u32>::new(); // block_leader -> latency

    let condensed_graph =
        condensate_graph(graph.clone(), &mut condensed_entry_node_latency, &blocks);

    let mut dot_file = std::fs::File::create("condensed_graph.dot").expect("Unable to create file");
    let digraph = condensed_graph.to_dot_graph();
    dot_file
        .write_all(digraph.as_bytes())
        .expect("Unable to write dot file");

    // find all the entry nodes
    let condensed_graph_nodes = condensed_graph.get_nodes();
    let entry_nodes = condensed_graph_nodes
        .iter()
        .filter(|node| condensed_graph.edges_directed(node, Incoming).is_empty())
        .collect::<Vec<_>>();

    let mut wcet: u32 = 0;
    for entry_node in entry_nodes {
        let entry_node_latency = match condensed_entry_node_latency.get(&entry_node[0].leader) {
            Some(latency) => *latency as u32,
            None => entry_node[0].get_latency(),
        };

        println!("entry node latency: {}", entry_node_latency);

        let max_path_latency = condensed_graph.longest_path(entry_node).unwrap() as u32;

        println!("max path latency: {}", max_path_latency);

        wcet = wcet.max(entry_node_latency + max_path_latency);
    }

    println!("WCET: {} clock cycles", wcet);
}
