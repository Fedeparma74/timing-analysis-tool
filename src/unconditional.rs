use capstone::{Arch, Insn};

pub fn is_unconditional_jump(insn: &Insn, arch: Arch) -> bool {
    let op = insn.mnemonic().unwrap();

    match arch {
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
    }
}
