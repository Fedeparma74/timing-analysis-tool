use capstone::{Arch, Mode};

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
                mode: Mode::Thumb,
            },
            object::Architecture::Riscv64 => ArchMode {
                arch: Arch::RISCV,
                mode: Mode::RiscV64,
            },
            object::Architecture::Riscv32 => ArchMode {
                arch: Arch::RISCV,
                mode: Mode::RiscV32,
            },
            object::Architecture::Mips64 => ArchMode {
                arch: Arch::MIPS,
                mode: Mode::Mips64,
            },
            object::Architecture::Mips => ArchMode {
                arch: Arch::MIPS,
                mode: Mode::Mips32,
            },
            object::Architecture::PowerPc64 => ArchMode {
                arch: Arch::PPC,
                mode: Mode::Mode64,
            },
            object::Architecture::PowerPc => ArchMode {
                arch: Arch::PPC,
                mode: Mode::Mode32,
            },
            object::Architecture::Sparc64 => ArchMode {
                arch: Arch::SPARC,
                mode: Mode::V9,
            },
            _ => panic!("unsupported architecture"),
        }
    }
}
