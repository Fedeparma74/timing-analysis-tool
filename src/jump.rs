use capstone::{Arch, Insn, InsnDetail, InsnGroupType};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ExitJump {
    ConditionalRelative { taken: u64, not_taken: u64 },
    UnconditionalRelative(u64),
    ConditionalAbsolute { taken: u64, not_taken: u64 },
    UnconditionalAbsolute(u64),
    Indirect,
    Ret(u64),
    Call(u64, u64),
    Next(u64),
}

impl std::fmt::Display for ExitJump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitJump::ConditionalRelative { taken, not_taken } => {
                write!(
                    f,
                    "ConditionalRelative {{ taken: 0x{taken:x}, not_taken: 0x{not_taken:x} }}"
                )
            }
            ExitJump::UnconditionalRelative(target) => {
                write!(f, "UnconditionalRelative {{ target: 0x{target:x} }}")
            }
            ExitJump::ConditionalAbsolute { taken, not_taken } => {
                write!(
                    f,
                    "ConditionalAbsolute {{ taken: 0x{taken:x}, not_taken: 0x{not_taken:x} }}"
                )
            }
            ExitJump::UnconditionalAbsolute(target) => {
                write!(f, "UnconditionalAbsolute {{ target: 0x{target:x} }}")
            }
            ExitJump::Indirect => write!(f, "Indirect"),
            ExitJump::Ret(targets) => {
                if *targets != 0 {
                    write!(f, "Ret {{ targets: 0x{targets:x} }}")
                } else {
                    write!(f, "Ret {{ targets: None }}")
                }
            }
            ExitJump::Call(target, _) => write!(f, "Call {{ target: 0x{target:x} }}"),
            ExitJump::Next(target) => write!(f, "Next {{ target: 0x{target:x} }}"),
        }
    }
}

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
            Arch::ARM => matches!(op, "b" | "bl" | "br" | "bx" | "blr" | "bcc" | "ret"),
            Arch::ARM64 => matches!(op, "b" | "bl" | "br" | "blr" | "bcc" | "ret"),
            Arch::MIPS => matches!(op, "j" | "jal" | "jr" | "jalr"),
            Arch::X86 => matches!(op, "jmp" | "call" | "ret"),
            Arch::PPC => matches!(op, "b" | "bl" | "blr" | "bctr" | "bctrl"),
            Arch::SPARC => matches!(
                op,
                "bpa" | "fbpa" | "call" | "ret" | "retl" | "rett" | "jmp" | "jmpl"
            ),
            Arch::RISCV => matches!(
                op,
                "j" | "jal"
                    | "jr"
                    | "jalr"
                    | "tail"
                    | "call"
                    | "ecall"
                    | "scall"
                    | "ret"
                    | "eret"
                    | "c.j"
                    | "c.jal"
                    | "c.jr"
                    | "c.jalr"
            ),
            _ => panic!("Unsupported architecture!"),
        };

        let operands = insn.op_str().unwrap();
        let operands = operands.split(',').collect::<Vec<&str>>();
        let last_operand = operands.last().unwrap().trim();

        if last_operand.contains("0x") {
            let last_operand =
                u64::from_str_radix(last_operand.split("0x").collect::<Vec<&str>>()[1], 16)
                    .unwrap();

            if is_call {
                return Some(ExitJump::Call(last_operand, next_insn.address()));
            }

            match (is_relative, is_unconditional) {
                (true, true) => Some(ExitJump::UnconditionalRelative(last_operand)),
                (true, false) => Some(ExitJump::ConditionalRelative {
                    taken: last_operand,
                    not_taken: next_insn.address(),
                }),
                (false, true) => Some(ExitJump::UnconditionalAbsolute(last_operand)),
                (false, false) => Some(ExitJump::ConditionalAbsolute {
                    taken: last_operand,
                    not_taken: next_insn.address(),
                }),
            }
        } else if is_ret {
            Some(ExitJump::Ret(0)) // the correct value can't be determined here
        } else {
            Some(ExitJump::Indirect)
        }
    } else {
        None
    }
}
