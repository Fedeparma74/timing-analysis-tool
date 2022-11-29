use crate::ExitJump;
use capstone::{Arch, Insn, InsnDetail, InsnGroupType};

pub fn get_exit_jump(insn: &Insn, insn_detail: &InsnDetail, arch: Arch) -> Option<ExitJump> {
    let insn_group_ids = insn_detail.groups();

    // check if the instruction is a jump and check its JumpType
    let mut is_jump = false;
    let mut is_relative = false;

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
        let op = insn.mnemonic().unwrap();
        let is_unconditional = match arch {
            // Arch::ARM => match op {
            //     "b" | "bl" | "bx" | "blx" => true,
            //     _ => false,
            // },
            Arch::ARM64 => matches!(op, "b" | "bl" | "br" | "blr" | "ret"),
            // Arch::MIPS => match op {
            //     "j" | "jal" | "jr" | "jalr" => true,
            //     _ => false,
            // },
            // Arch::X86 => match op {
            //     "jmp" | "ret" | "call" => true,
            //     _ => false,
            // },
            // Arch::PPC => match op {
            //     "b" | "bl" | "blr" | "bctr" | "bctrl" => true,
            //     _ => false,
            // },
            Arch::SPARC => todo!(),
            Arch::SYSZ => todo!(),
            Arch::XCORE => todo!(),
            Arch::M68K => todo!(),
            Arch::TMS320C64X => todo!(),
            Arch::M680X => todo!(),
            Arch::EVM => todo!(),
            // Arch::RISCV => match op {
            //     "j" | "jal" | "jr" | "jalr" | "ret" | _ => false,
            // },
            _ => todo!(),
        };

        let operands = insn.op_str().unwrap();
        let operands = operands.split(',').collect::<Vec<&str>>();
        let last_operand = operands.last().unwrap().trim();

        if last_operand.starts_with("#0x") {
            let last_operand =
                u64::from_str_radix(last_operand.trim_start_matches("#0x"), 16).unwrap();

            match (is_relative, is_unconditional) {
                (true, true) => Some(ExitJump::UnconditionalRelative(
                    insn.address() + last_operand,
                )),
                (true, false) => Some(ExitJump::ConditionalRelative {
                    taken: insn.address() + last_operand,
                    not_taken: insn.address() + insn.bytes().len() as u64,
                }),
                (false, true) => Some(ExitJump::UnconditionalAbsolute(last_operand)),
                (false, false) => Some(ExitJump::ConditionalAbsolute {
                    taken: last_operand,
                    not_taken: insn.address() + insn.bytes().len() as u64,
                }),
            }
        } else {
            Some(ExitJump::Indirect)
        }
    } else {
        None
    }
}
