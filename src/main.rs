mod unconditional;

use std::io::Write;
use std::{collections::HashSet, fmt::Display};

use object::{Object, ObjectSection};

use capstone::{Arch, Insn, InsnGroupType, Mode, NO_EXTRA_MODE};
use unconditional::is_unconditional_jump;

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

    let mut rel_file = std::fs::File::create("relative.txt").expect("Unable to create file");

    //for iteration to find all leaders
    for (index, insn) in insns.iter().enumerate() {
        let insn_detail = cs.insn_detail(insn).unwrap();
        let insn_group_ids = insn_detail.groups();

        // check if the instruction is a jump and check its JumpType
        let mut is_jump = false;
        let mut is_relative = false;
        let mut jump_type: Option<JumpType> = None;

        for id in insn_group_ids {
            let id = id.0 as u32;

            if id == InsnGroupType::CS_GRP_CALL
                || id == InsnGroupType::CS_GRP_INT
                || id == InsnGroupType::CS_GRP_JUMP
                || id == InsnGroupType::CS_GRP_RET
                || id == InsnGroupType::CS_GRP_IRET
            {
                is_jump = true;
            } else if id == InsnGroupType::CS_GRP_BRANCH_RELATIVE {
                is_relative = true;
            }
        }

        if is_jump {
            if let Some(operands) = insn.op_str() {
                let operands = operands.split(',').collect::<Vec<&str>>();
                let last_operand = operands.last().unwrap().trim();
                if last_operand.starts_with("#0x") {
                    let last_operand =
                        u64::from_str_radix(last_operand.trim_start_matches("#0x"), 16).unwrap();
                    if is_relative {
                        jump_type = Some(JumpType::Relative(last_operand));
                    } else {
                        jump_type = Some(JumpType::Absolute(last_operand));
                    }
                } else {
                    jump_type = Some(JumpType::Indirect);
                }
            }
        }


        // check if the instruction is a leader and inserting the target address into the leaders Hash-set
        if let Some(jump) = jump_type {
            // insert next instruction as leader
            if index != insns.len() - 1 {
                leaders.insert(insn.address() + insn.bytes().len() as u64);
            }

            match jump {
                JumpType::Absolute(addr) => {
                    leaders.insert(addr);
                }
                JumpType::Relative(offset) => {
                    writeln!(rel_file, "0x{:x}", insn.address() + offset).unwrap();
                    leaders.insert(insn.address() + offset);
                }
                JumpType::Indirect => {}
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

        //added
        let insn_detail = cs.insn_detail(insn).unwrap();
        let insn_group_ids = insn_detail.groups();
        let mut is_jump = false;

        for id in insn_group_ids {
            let id = id.0 as u32;

            if id == InsnGroupType::CS_GRP_CALL
                || id == InsnGroupType::CS_GRP_INT
                || id == InsnGroupType::CS_GRP_JUMP
                || id == InsnGroupType::CS_GRP_RET
                || id == InsnGroupType::CS_GRP_IRET
                || id == InsnGroupType::CS_GRP_BRANCH_RELATIVE
            {
                is_jump = true;
            } 
        }
        // end added

        // if the next instruction is a leader, push the current block to the list of blocks
        if leaders.contains(&next_insn.address()) {
            // set the exit jump type of the current block
            if is_unconditional_jump(insn, arch_mode.arch) {
                current_block.set_exit_jump(ExitJump::Unconditional);
            } else  if is_jump {  //modified
                current_block.set_exit_jump(ExitJump::Conditional);
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
enum JumpType {
    Relative(u64), // pc + offset
    Absolute(u64), // address
    Indirect,      // register
}

#[derive(Debug, Clone)]
pub enum ExitJump {
    Conditional,
    Unconditional,
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
