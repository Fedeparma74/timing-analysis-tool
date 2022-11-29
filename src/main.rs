mod jump;

use std::collections::HashMap;
use std::io::Write;
use std::{collections::HashSet, fmt::Display};

use object::{Object, ObjectSection};

use capstone::{Arch, Insn, Mode, NO_EXTRA_MODE};
use jump::get_exit_jump;

fn main() {
    let bin_file = std::fs::read("calculator").unwrap();
    let obj_file = object::File::parse(bin_file.as_slice()).unwrap();
    let arch = obj_file.architecture();
    let code = obj_file
        .section_by_name(".text")
        .expect(".text section not available")
        .data()
        .unwrap();

    let arch_mode = ArchMode::from(arch);

    let mut cs = capstone::Capstone::new_raw(arch_mode.arch, arch_mode.mode, NO_EXTRA_MODE, None)
        .unwrap_or_else(|e| panic!("Failed to create Capstone handle: {}", e));
    cs.set_detail(true).unwrap();

    let insns = cs
        .disasm_all(code, 0x1000)
        .unwrap_or_else(|e| panic!("Failed to disassemble given code: {}", e));

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
    let mut current_block: Block = Block::new();

    // for each window of 2 instructions
    insns.windows(2).enumerate().for_each(|(index, window)| {
        let insn = &window[0];
        let next_insn = &window[1];

        // push the instruction to the current block
        current_block.add_instruction(insn);

        // if the next instruction is a leader, push the current block to the list of blocks
        if leaders.contains(&next_insn.address()) {
            if let Some(exit_jump) = jumps.get(&insn.address()) {
                current_block.set_exit_jump(exit_jump.clone());
            }

            blocks.push(current_block.clone());
            current_block = Block::new();
        }

        // last instruction pair -> add last instruction to block and push block (exit_jump is None)
        if index == insns.len() - 2 {
            current_block.add_instruction(next_insn);
            blocks.push(current_block.clone());
        }
    });

    let mut file = std::fs::File::create("output.txt").expect("Unable to create file");

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
    pub exit_jump: Option<ExitJump>,
}

impl<'a> Block<'a> {
    pub fn new() -> Self {
        Self::default()
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
