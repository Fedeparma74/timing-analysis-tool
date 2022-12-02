use crate::ExitJump;
use capstone::{Arch, Insn, InsnDetail, InsnGroupType};

pub fn get_exit_jump(
    insn: &Insn,
    next_insn: &Insn,
    insn_detail: &InsnDetail,
    arch: Arch,
) -> Option<ExitJump> {
    let insn_group_ids = insn_detail.groups();

    // check if the instruction is a jump and check its JumpType
    let mut is_jump = false;
    let mut is_relative = false;
    let mut is_call = false;
    let mut is_ret = false;

    for id in insn_group_ids {
        let id = id.0 as u32;

        if id == InsnGroupType::CS_GRP_INT
            || id == InsnGroupType::CS_GRP_CALL
            || id == InsnGroupType::CS_GRP_JUMP
            || id == InsnGroupType::CS_GRP_RET
            || id == InsnGroupType::CS_GRP_IRET
        {
            is_jump = true;

            if id == InsnGroupType::CS_GRP_CALL {
                is_call = true;
            } else if id == InsnGroupType::CS_GRP_RET {
                is_ret = true;
            }
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
            Arch::X86 => matches!(op, "jmp" | "call" | "ret"),
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

        if last_operand.contains("0x") {
            let last_operand =
                u64::from_str_radix(last_operand.split("0x").collect::<Vec<&str>>()[1], 16)
                    .unwrap();

            if is_call {
                return Some(ExitJump::Call(last_operand));
            }

            match (is_relative, is_unconditional) {
                (true, true) => Some(ExitJump::UnconditionalRelative(
                    insn.address() + last_operand,
                )),
                (true, false) => Some(ExitJump::ConditionalRelative {
                    taken: insn.address() + last_operand,
                    not_taken: next_insn.address(),
                }),
                (false, true) => Some(ExitJump::UnconditionalAbsolute(last_operand)),
                (false, false) => Some(ExitJump::ConditionalAbsolute {
                    taken: last_operand,
                    not_taken: next_insn.address(),
                }),
            }
        } else if is_ret {
            // return the last value of lastcalls and remove it from the vector
            Some(ExitJump::Ret(None))
        } else {
            Some(ExitJump::Indirect)
        }
    } else {
        None
    }
}
