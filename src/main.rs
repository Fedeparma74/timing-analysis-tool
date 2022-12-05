mod block;
mod jump;

use std::collections::{hash_map, HashMap};
use std::io::Write;
use std::{collections::HashSet, fmt::Display};

use capstone::{Arch, Insn, Mode, NO_EXTRA_MODE};
use jump::get_exit_jump;
use object::{Object, ObjectSection};
use petgraph::algo::{bellman_ford, condensation};
use petgraph::dot::Dot;
use petgraph::graph::NodeIndex;
use petgraph::stable_graph::StableGraph;
use petgraph::visit::EdgeRef;
use petgraph::Direction::{Incoming, Outgoing};

use crate::block::Block;

const MAX_CYCLES: u32 = 1;

fn main() {
    let bin_file = std::fs::read("cyclic.o").unwrap();
    let obj_file = object::File::parse(bin_file.as_slice()).unwrap();
    let arch = obj_file.architecture();

    let mut text_data = Vec::new();
    // print file sections
    for section in obj_file.sections() {
        // join all the sections .text in one
        println!("Section: {}", section.name().unwrap());
        if section.name().unwrap().contains("text") {
            text_data.extend_from_slice(section.data().unwrap());
        }
    }

    let arch_mode = ArchMode::from(arch);

    println!("{:?}", arch_mode);

    let mut cs = capstone::Capstone::new_raw(arch_mode.arch, arch_mode.mode, NO_EXTRA_MODE, None)
        .unwrap_or_else(|e| panic!("Failed to create Capstone handle: {}", e));
    cs.set_detail(true).unwrap();

    let instructions = cs
        .disasm_all(&text_data, 0x1000)
        .unwrap_or_else(|e| panic!("Failed to disassemble given code: {}", e));

    let mut leaders = HashSet::new();
    let mut jumps: HashMap<u64, ExitJump> = HashMap::new(); // jump_address -> ExitJump
    let mut call_map = HashMap::<u64, Vec<u64>>::new(); // call_target_address -> return_addresses (ret)
    let mut node_index_map = HashMap::<u64, NodeIndex<u32>>::new(); // block leader -> node index

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
                ExitJump::Call(target) => {
                    if next_instruction.address() != target {
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

    let mut graph = StableGraph::<Block, f32>::new();

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

                let node_index = graph.add_node(current_block.clone());
                node_index_map.insert(current_block.leader, node_index);
                current_block = Block::new(next_insn.into());
            } else {
                // push the instruction to the current block
                current_block.add_instruction(next_insn.into());
            }

            // last instruction pair -> add last instruction to block and push block (exit_jump is None)
            if index == instructions.len() - 2 {
                current_block.add_instruction(next_insn.into());
                // blocks.push(current_block.clone());
                let i = graph.add_node(current_block.clone());
                node_index_map.insert(current_block.leader, i);
            }
        });

    // add edges to the graph
    for block in graph.clone().node_weights() {
        for target in block.get_targets() {
            let next_block = graph
                .node_weight(node_index_map.get(&target).unwrap().clone())
                .unwrap();
            graph.add_edge(
                node_index_map.get(&block.leader).unwrap().clone(),
                node_index_map.get(&target).unwrap().clone(),
                next_block.get_latency() as f32,
            );
        }
    }

    let mut file = std::fs::File::create("output.txt").expect("Unable to create file");
    let mut dot_file = std::fs::File::create("graph.dot").expect("Unable to create file");

    for block in graph.node_weights() {
        // write output to txt file
        writeln!(file, "Block:\n{}", block).unwrap();
    }

    let digraph = Dot::with_config(&graph, &[]);

    dot_file
        .write_all(digraph.to_string().as_bytes())
        .expect("Unable to write dot file");

    let mut condensed_graph = condensation(graph.clone().into(), true);
    let mut condensed_graph_map = HashMap::<u64, NodeIndex<u32>>::new();

    for condensed_node_index in condensed_graph.node_indices() {
        let blocks = condensed_graph.node_weight(condensed_node_index).unwrap();
        // populate the condensed_node_index_map
        for block in blocks.iter() {
            condensed_graph_map.insert(block.leader, condensed_node_index);
        }
    }

    for condensed_node_index in condensed_graph.node_indices() {
        let blocks = condensed_graph.node_weight(condensed_node_index).unwrap();
        if blocks.len() > 1 {
            // if the node has more than one block, it is a cyclic node that has been condensed

            // create new graph with the blocks of the condensed node, acyclic
            let mut cycle_graph = StableGraph::<Block, f32>::new();
            let mut condensed_node_index_map = HashMap::<u64, NodeIndex<u32>>::new();

            // add nodes to the cycle_graph
            for block in blocks.iter() {
                let i = cycle_graph.add_node(block.clone());
                condensed_node_index_map.insert(block.leader, i);
            }

            // add edges to the cycle_graph
            for block in cycle_graph.clone().node_weights() {
                for target in block.get_targets() {
                    if let Some(target_node_index) = condensed_node_index_map.get(&target) {
                        let next_block =
                            cycle_graph.node_weight(target_node_index.clone()).unwrap();
                        cycle_graph.add_edge(
                            condensed_node_index_map.get(&block.leader).unwrap().clone(),
                            target_node_index.clone(),
                            next_block.get_latency() as f32,
                        );
                    }
                }
            }

            // remove incoming edge of entry node
            let cycle_entry_block = blocks[0].clone();
            let cycle_entry_block_index = condensed_node_index_map
                .get(&cycle_entry_block.leader)
                .unwrap();

            for edge in cycle_graph
                .clone()
                .edges_directed(*cycle_entry_block_index, Incoming)
            {
                cycle_graph.remove_edge(edge.id());
            }

            let digraph = Dot::with_config(&cycle_graph, &[]);
            let mut dot_file =
                std::fs::File::create("graph_cycle.dot").expect("Unable to create file");
            dot_file
                .write_all(digraph.to_string().as_bytes())
                .expect("Unable to write dot file");

            // find the longest path in the cycle graph
            let path = bellman_ford(&cycle_graph, *cycle_entry_block_index).unwrap();

            // calculate the latency of the longest path
            let max_path_latency = path
                .distances
                .iter()
                .filter(|x| x.is_finite())
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap()
                .to_owned() as u32;

            // calculate the total latency of the cycle
            let cycle_latency = cycle_entry_block.get_latency() + max_path_latency;

            // get the outer block of the cyclic node (it's always only one because it's the exit condition of the cycle)
            let outer_block_index = condensed_graph
                .neighbors_directed(condensed_node_index, Outgoing)
                .next()
                .unwrap();
            let outer_block = &condensed_graph.node_weight(outer_block_index).unwrap()[0];

            // get the outer block in the original graph
            let origin_outer_block_index = node_index_map.get(&outer_block.leader).unwrap().clone();

            // get the cycle exit block in the original graph
            let exit_block_index = graph
                .neighbors_directed(origin_outer_block_index, Incoming)
                .next()
                .unwrap();
            let exit_block = graph.node_weight(exit_block_index).unwrap();

            let condensed_exit_block_index = condensed_node_index_map
                .get(&exit_block.leader)
                .unwrap()
                .clone();

            let direct_path_latency = cycle_latency
                - bellman_ford(&cycle_graph, condensed_exit_block_index)
                    .unwrap()
                    .distances
                    .iter()
                    .filter(|x| x.is_finite())
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap()
                    .to_owned() as u32;

            let total_cycle_latency = direct_path_latency + MAX_CYCLES * cycle_latency;

            // edit the outgoing edge latency of the condensed node
            for edge in condensed_graph
                .clone()
                .edges_directed(condensed_node_index, Incoming)
            {
                // let edge_index = graph_cycle.find_edge(edge.id(), edge.target()).unwrap();
                condensed_graph.update_edge(
                    edge.source(),
                    edge.target(),
                    total_cycle_latency as f32,
                );
            }
        }
    }

    // find all the entry nodes
    let entry_nodes = condensed_graph
        .node_indices()
        .filter(|node| condensed_graph.edges_directed(*node, Incoming).count() == 0)
        .collect::<Vec<_>>();

    let mut wcet: u32 = 0;
    for entry_node in entry_nodes {
        let entry_node_latency = condensed_graph.node_weight(entry_node).unwrap()[0].get_latency();
        let path = bellman_ford(&condensed_graph, entry_node).unwrap();
        let max_path_latency = path
            .distances
            .iter()
            .filter(|x| x.is_finite())
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap()
            .to_owned() as u32;

        wcet = wcet.max(entry_node_latency + max_path_latency);
    }

    println!("WCET: {} clock cycles", wcet);

    let mut dot_file = std::fs::File::create("condensed_graph.dot").expect("Unable to create file");
    let digraph = Dot::with_config(&condensed_graph, &[]);
    dot_file
        .write_all(format!("{:?}", digraph).as_bytes())
        .expect("Unable to write dot file");
}

#[derive(Debug, Clone)]
pub enum ExitJump {
    ConditionalRelative { taken: u64, not_taken: u64 },
    UnconditionalRelative(u64),
    ConditionalAbsolute { taken: u64, not_taken: u64 },
    UnconditionalAbsolute(u64),
    Indirect,
    Ret(Vec<u64>),
    Call(u64),
    Next(u64),
}

impl Display for ExitJump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitJump::ConditionalRelative { taken, not_taken } => {
                write!(
                    f,
                    "ConditionalRelative {{ taken: 0x{:x}, not_taken: 0x{:x} }}",
                    taken, not_taken
                )
            }
            ExitJump::UnconditionalRelative(target) => {
                write!(f, "UnconditionalRelative {{ target: 0x{:x} }}", target)
            }
            ExitJump::ConditionalAbsolute { taken, not_taken } => {
                write!(
                    f,
                    "ConditionalAbsolute {{ taken: 0x{:x}, not_taken: 0x{:x} }}",
                    taken, not_taken
                )
            }
            ExitJump::UnconditionalAbsolute(target) => {
                write!(f, "UnconditionalAbsolute {{ target: 0x{:x} }}", target)
            }
            ExitJump::Indirect => write!(f, "Indirect"),
            ExitJump::Ret(targets) => {
                if targets.is_empty() {
                    write!(f, "Ret {{ targets: None }}")
                } else {
                    write!(f, "Ret {{ targets: [")?;
                    for target in targets {
                        write!(f, "0x{:x}, ", target)?;
                    }
                    write!(f, "] }}")
                }
            }
            ExitJump::Call(target) => write!(f, "Call {{ target: 0x{:x} }}", target),
            ExitJump::Next(target) => write!(f, "Next {{ target: 0x{:x} }}", target),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub address: u64,
    pub mnemonic: String,
    pub operands: (Option<String>, Option<String>),
    pub latency: u32, // in cycles
}

impl<'a> From<&'a Insn<'a>> for Instruction {
    fn from(insn: &'a Insn<'a>) -> Self {
        let operands = match insn.op_str() {
            Some(operands) => {
                let operands = operands.split(',').map(|o| o.trim()).collect::<Vec<&str>>();
                if operands.len() == 1 {
                    (Some(operands[0]), None)
                } else {
                    (Some(operands[0]), Some(operands[1]))
                }
            }
            None => (None, None),
        };
        Instruction {
            address: insn.address(),
            mnemonic: insn.mnemonic().unwrap().to_string(),
            operands: (
                operands.0.map(|s| s.to_string()),
                operands.1.map(|s| s.to_string()),
            ),
            latency: 1, // TODO: get the latency from the instruction
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let operands_str = match &self.operands {
            (Some(op1), Some(op2)) => format!("{}, {}", op1, op2),
            (Some(op1), None) => format!("{}", op1),
            _ => "".to_string(),
        };
        write!(
            f,
            "0x{:x} {} {}",
            self.address,
            self.mnemonic,
            operands_str.trim()
        )
    }
}

#[derive(Debug, Clone)]
pub struct ArchMode {
    pub arch: Arch,
    pub mode: Mode,
}

impl From<object::Architecture> for ArchMode {
    fn from(value: object::Architecture) -> Self {
        match value {
            object::Architecture::X86_64 => ArchMode {
                arch: Arch::X86,
                mode: Mode::Mode64,
            },
            object::Architecture::X86_64_X32 => ArchMode {
                arch: Arch::X86,
                mode: Mode::Mode32,
            },
            object::Architecture::Aarch64 => ArchMode {
                arch: Arch::ARM64,
                mode: Mode::Arm,
            },
            object::Architecture::Arm => ArchMode {
                arch: Arch::ARM,
                mode: Mode::Mode32,
            },
            object::Architecture::LoongArch64 => ArchMode {
                arch: Arch::ARM,
                mode: Mode::Mode64,
            },
            _ => panic!("unsupported architecture"),
        }
    }
}
