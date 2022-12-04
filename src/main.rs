mod jump;

use std::collections::{hash_map, HashMap};
use std::io::Write;
use std::{collections::HashSet, fmt::Display};

use capstone::{Arch, Insn, Mode, NO_EXTRA_MODE};
use jump::get_exit_jump;
use object::{Object, ObjectSection};
use petgraph::algo::{bellman_ford, condensation};
use petgraph::dot::{Config, Dot};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction::{Incoming, Outgoing};
use petgraph::Graph;

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

    println!("Text section size: {}", text_data.len());

    let arch_mode = ArchMode::from(arch);

    println!("{:?}", arch_mode);

    let mut cs = capstone::Capstone::new_raw(arch_mode.arch, arch_mode.mode, NO_EXTRA_MODE, None)
        .unwrap_or_else(|e| panic!("Failed to create Capstone handle: {}", e));
    cs.set_detail(true).unwrap();

    let instructions = cs
        .disasm_all(&text_data, 0x1000)
        .unwrap_or_else(|e| panic!("Failed to disassemble given code: {}", e));

    println!("{}", instructions.len());

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
                ExitJump::Ret(_) => {}
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
                ExitJump::Next(_) => {}
            }
        }
    });

    // iterate through all instructions and create the basic blocks
    let mut current_block: Block = Block::new(&instructions[0]);

    let mut graph = Graph::<Block, f32>::new();

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
                                call_map.get(&current_block.leader).cloned(),
                            ));
                        }
                    } else {
                        current_block.set_exit_jump(exit_jump.clone());
                    }
                } else {
                    current_block.set_exit_jump(ExitJump::Next(next_insn.address()));
                }

                let i = graph.add_node(current_block.clone());
                node_index_map.insert(current_block.leader, i);
                current_block = Block::new(next_insn);
            } else {
                // push the instruction to the current block
                current_block.add_instruction(next_insn);
            }

            // last instruction pair -> add last instruction to block and push block (exit_jump is None)
            if index == instructions.len() - 2 {
                current_block.add_instruction(next_insn);
                // blocks.push(current_block.clone());
                let i = graph.add_node(current_block.clone());
                node_index_map.insert(current_block.leader, i);
            }
        });

    // add edges to the graph
    for block in graph.clone().node_weights() {
        for edge in block.get_edges() {
            let next_block = graph
                .node_weight(node_index_map.get(&edge.1).unwrap().clone())
                .unwrap();
            graph.add_edge(
                node_index_map.get(&edge.0).unwrap().clone(),
                node_index_map.get(&edge.1).unwrap().clone(),
                next_block.get_latency(),
            );
        }
    }

    let mut file = std::fs::File::create("output.txt").expect("Unable to create file");
    let mut dot_file = std::fs::File::create("graph.dot").expect("Unable to create file");

    let blocks = graph.raw_nodes();
    for block in blocks {
        // write output to txt file
        writeln!(file, "Block:\n{}", block.weight).unwrap();
    }

    let digraph = Dot::with_config(&graph, &[Config::EdgeNoLabel]);

    dot_file
        .write_all(digraph.to_string().as_bytes())
        .expect("Unable to write dot file");

    let mut condensed_graph = condensation(graph.clone(), true);
    let mut condensed_node_index_map = HashMap::<u64, NodeIndex<u32>>::new();

    for cyclic_node in condensed_graph.node_indices() {
        let blocks = condensed_graph.node_weight(cyclic_node).unwrap();
        //aggiungo i nodi
        for block in blocks.iter() {
            condensed_node_index_map.insert(block.leader, cyclic_node);
        }
    }

    for cyclic_node in condensed_graph.node_indices() {
        let blocks = condensed_graph.node_weight(cyclic_node).unwrap();
        if blocks.len() > 1 {
            // nodo condensato

            //creo un nuovo grafo
            let mut graph_cycle = Graph::<Block, f32>::new();
            let mut cyclic_node_index_map = HashMap::<u64, NodeIndex<u32>>::new();

            //aggiungo i nodi
            for block in blocks.iter() {
                let i = graph_cycle.add_node(block.clone());
                cyclic_node_index_map.insert(block.leader, i);
            }

            //aggiungo gli archi
            for block in graph_cycle.clone().node_weights() {
                for edge in block.get_edges() {
                    if let Some(target) = cyclic_node_index_map.get(&edge.1) {
                        let next_block = graph_cycle.node_weight(target.clone()).unwrap();
                        graph_cycle.add_edge(
                            cyclic_node_index_map.get(&edge.0).unwrap().clone(),
                            target.clone(),
                            next_block.get_latency(),
                        );
                    }
                }
            }

            // remove incoming edge of entry node
            let entry_node_index = cyclic_node_index_map.get(&blocks[0].leader).unwrap();

            println!("Entry node index: {:?}", entry_node_index);

            for edge in graph_cycle
                .clone()
                .edges_directed(*entry_node_index, Incoming)
            {
                // let edge_index = graph_cycle.find_edge(edge.id(), edge.target()).unwrap();
                graph_cycle.remove_edge(edge.id());
            }

            //stampa del cycle_graph
            let digraph = Dot::with_config(&graph_cycle, &[Config::EdgeNoLabel]);

            let mut dot_file =
                std::fs::File::create("graph_cycle.dot").expect("Unable to create file");

            dot_file
                .write_all(digraph.to_string().as_bytes())
                .expect("Unable to write dot file");

            //cerco percorso critico
            let path = bellman_ford(&graph_cycle, *entry_node_index).unwrap();
            // println!("Path: {:#?}", path);

            // entry node latency
            let entry_node_latency = graph_cycle
                .node_weight(*entry_node_index)
                .unwrap()
                .get_latency();

            // calcolo latenza del percorso critico
            let max_path_latency = path
                .distances
                .iter()
                .filter(|x| x.is_finite())
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap()
                .to_owned();

            let cycle_latency = entry_node_latency + max_path_latency;

            // get the outgoing block of the cyclic node (it's always only one because it's the exit condition of the cycle)
            let outer_block_index = condensed_graph
                .neighbors_directed(cyclic_node, Outgoing)
                .next()
                .unwrap();

            let outer_block = &condensed_graph.node_weight(outer_block_index).unwrap()[0];

            let exit_block_index = node_index_map.get(&outer_block.leader).unwrap().clone();
            let exit_block = graph.node_weight(exit_block_index).unwrap();

            let condensed_exit_block_index = condensed_node_index_map
                .get(&exit_block.leader)
                .unwrap()
                .clone();

            let direct_path_latency = cycle_latency
                - bellman_ford(&graph_cycle, condensed_exit_block_index)
                    .unwrap()
                    .distances
                    .iter()
                    .filter(|x| x.is_finite())
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap()
                    .to_owned();

            let total_cycle_latency = direct_path_latency + MAX_CYCLES as f32 * cycle_latency;

            // edit the outgoing edge latency of the condensed node
            for edge in condensed_graph
                .clone()
                .edges_directed(cyclic_node, Incoming)
            {
                // let edge_index = graph_cycle.find_edge(edge.id(), edge.target()).unwrap();
                condensed_graph.update_edge(edge.source(), edge.target(), total_cycle_latency);
            }
        }
    }

    // find all the entry nodes
    let entry_nodes = condensed_graph
        .node_indices()
        .filter(|node| condensed_graph.edges_directed(*node, Incoming).count() == 0)
        .collect::<Vec<_>>();

    let mut wcet: f32 = 0.0;
    for entry_node in entry_nodes {
        let entry_node_latency = condensed_graph.node_weight(entry_node).unwrap()[0].get_latency();
        println!("Entry node latency: {}", entry_node_latency);
        let path = bellman_ford(&condensed_graph, entry_node).unwrap();
        println!("Path: {:#?}", path);
        let max_path_latency = path
            .distances
            .iter()
            .filter(|x| x.is_finite())
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap()
            .to_owned();

        wcet = wcet.max(entry_node_latency + max_path_latency);
    }

    println!("WCET: {}", wcet);

    let mut dot_file = std::fs::File::create("condensed_graph.dot").expect("Unable to create file");

    let digraph = Dot::with_config(&condensed_graph, &[Config::EdgeIndexLabel]);

    // println!("Condensed graph: {:?}", digraph);

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
    Ret(Option<Vec<u64>>),
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
            ExitJump::Ret(target) => {
                if let Some(target) = target {
                    write!(
                        f,
                        "Ret {{ targets: {} }}",
                        target
                            .iter()
                            .map(|x| format!("0x{:x}", x))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    write!(f, "Ret {{ target: None }}")
                }
            }
            ExitJump::Call(target) => write!(f, "Call {{ target: 0x{:x} }}", target),
            ExitJump::Next(target) => write!(f, "Next {{ target: 0x{:x} }}", target),
        }
    }
}

#[derive(Clone, Default)]
pub struct Block<'a> {
    pub instructions: Vec<&'a Insn<'a>>,
    pub leader: u64,
    pub exit_jump: Option<ExitJump>,
}

impl<'a> Block<'a> {
    pub fn new(instruction: &'a Insn<'a>) -> Self {
        Self {
            instructions: vec![instruction],
            leader: instruction.address(),
            exit_jump: None,
        }
    }

    pub fn add_instruction(&mut self, instruction: &'a Insn<'a>) {
        self.instructions.push(instruction);
    }

    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    pub fn set_exit_jump(&mut self, exit_jump: ExitJump) {
        self.exit_jump = Some(exit_jump);
    }

    pub fn get_edges(&self) -> Vec<(u64, u64)> {
        let mut edges = vec![];

        if let Some(exit_jump) = &self.exit_jump {
            match exit_jump {
                ExitJump::ConditionalRelative { taken, not_taken } => {
                    edges.push((self.leader, *taken));
                    edges.push((self.leader, *not_taken));
                }
                ExitJump::UnconditionalRelative(target) => {
                    edges.push((self.leader, *target));
                }
                ExitJump::ConditionalAbsolute { taken, not_taken } => {
                    edges.push((self.leader, *taken));
                    edges.push((self.leader, *not_taken));
                }
                ExitJump::UnconditionalAbsolute(target) => {
                    edges.push((self.leader, *target));
                }
                ExitJump::Indirect => {}
                ExitJump::Ret(target) => {
                    if let Some(target) = target {
                        for target in target {
                            edges.push((self.leader, *target));
                        }
                    }
                }
                ExitJump::Call(target) => {
                    edges.push((self.leader, *target));
                }
                ExitJump::Next(target) => {
                    edges.push((self.leader, *target));
                }
            }
        }

        edges
    }

    pub fn get_latency(&self) -> f32 {
        // TODO: real latency
        self.instructions.len() as f32
    }
}

impl Display for Block<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for insn in self.instructions.iter() {
            writeln!(f, "{}", insn)?;
        }
        if let Some(exit_jump) = &self.exit_jump {
            writeln!(f, "Exit jump: {}", exit_jump)?;
        } else {
            writeln!(f, "Exit jump: None")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Block<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for insn in self.instructions.iter() {
            writeln!(f, "{}", insn)?;
        }
        if let Some(exit_jump) = &self.exit_jump {
            writeln!(f, "Exit jump: {}", exit_jump)?;
        } else {
            writeln!(f, "Exit jump: None")?;
        }
        Ok(())
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
