mod graph;
mod jump;

use std::collections::HashMap;
use std::io::Write;
use std::{collections::HashSet, fmt::Display};

use graph::Edges;
use object::{Object, ObjectSection};

use capstone::{Arch, Insn, Mode, NO_EXTRA_MODE};
use jump::get_exit_jump;

fn main() {
    let bin_file = std::fs::read("prova.o").unwrap();
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

    let insns = cs
        .disasm_all(&text_data, 0x1000)
        .unwrap_or_else(|e| panic!("Failed to disassemble given code: {}", e));

    println!("{}", insns.len());

    let mut leaders = HashSet::new();
    let mut jumps: HashMap<u64, ExitJump> = HashMap::new(); // jump_address -> ExitJump

    // iteration to find all leaders and exit jumps
    for (index, insn) in insns.iter().enumerate() {
        let insn_detail = cs.insn_detail(insn).unwrap();

        let exit_jump = get_exit_jump(insn, &insn_detail, arch_mode.arch);

        // if the instruction is a jump, add the jump target address and the next instruction address to the leaders
        // Then add the jump instruction to the jumps map
        if let Some(exit_jump) = exit_jump {
            jumps.insert(insn.address(), exit_jump.clone());

            // insert next instruction as leader
            if index != insns.len() - 1 {
                leaders.insert(insn.address() + insn.bytes().len() as u64);
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
                ExitJump::Indirect => {}
            }
        }
    }

    // iterate through all instructions and create the basic blocks
    let mut blocks: Vec<Block> = Vec::new();
    let mut current_block: Block = Block::new(&insns[0]);

    // for each window of 2 instructions
    insns.windows(2).enumerate().for_each(|(index, window)| {
        let insn = &window[0];
        let next_insn = &window[1];

        // if the next instruction is a leader, push the current block to the list of blocks
        if leaders.contains(&next_insn.address()) {
            if let Some(exit_jump) = jumps.get(&insn.address()) {
                current_block.set_exit_jump(exit_jump.clone());
            }

            blocks.push(current_block.clone());
            current_block = Block::new(next_insn);
        } else {
            // push the instruction to the current block
            current_block.add_instruction(next_insn);
        }

        // last instruction pair -> add last instruction to block and push block (exit_jump is None)
        if index == insns.len() - 2 {
            current_block.add_instruction(next_insn);
            blocks.push(current_block.clone());
        }
    });

    let mut file = std::fs::File::create("output.txt").expect("Unable to create file");

    let edges = blocks
        .iter()
        .flat_map(|block| block.get_edges())
        .collect::<Vec<_>>();

    let mut dot_file = std::fs::File::create("graph.dot").expect("Unable to create file");

    dot::render(&Edges(edges), &mut dot_file).expect("Unable to write dot file");

    // create dot graph file
    for block in blocks {
        // write output to txt file
        writeln!(file, "Block:\n{}", block).unwrap();
    }
}

#[derive(Debug, Clone)]
pub enum ExitJump {
    ConditionalRelative { taken: u64, not_taken: u64 },
    UnconditionalRelative(u64),
    ConditionalAbsolute { taken: u64, not_taken: u64 },
    UnconditionalAbsolute(u64),
    Indirect,
}

#[derive(Debug, Clone, Default)]
pub struct Block<'a> {
    pub insns: Vec<&'a Insn<'a>>,
    pub leader: u64,
    pub exit_jump: Option<ExitJump>,
}

impl<'a> Block<'a> {
    pub fn new(insn: &'a Insn<'a>) -> Self {
        Self {
            insns: vec![insn],
            leader: insn.address(),
            exit_jump: None,
        }
    }

    pub fn add_instruction(&mut self, insn: &'a Insn<'a>) {
        self.insns.push(insn);
    }

    pub fn is_empty(&self) -> bool {
        self.insns.is_empty()
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
            }
        }

        edges
    }
}

impl Display for Block<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for insn in self.insns.iter() {
            writeln!(f, "{}", insn)?;
        }
        writeln!(f, "Exit jump: {:?}", self.exit_jump)?;
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
