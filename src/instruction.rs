use capstone::Insn;

use crate::CURRENT_ARCH;

#[derive(Debug, Clone)]
pub struct Instruction {
    pub address: u64,
    pub mnemonic: String,
    pub operands: (Option<String>, Option<String>),
    pub latency: u32, // in cycles
}

impl<'a> From<&'a Insn<'a>> for Instruction {
    fn from(insn: &'a Insn<'a>) -> Self {
        let mnemonic = insn.mnemonic().unwrap().to_string();

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

        let arch_mode = CURRENT_ARCH.with(|arch| arch.borrow().clone());

        let arch_mnemonic_str = if let Some(arch_mode) = arch_mode {
            format!(
                "{}_{}",
                arch_mode.arch.to_string().to_uppercase(),
                mnemonic.to_uppercase()
            )
        } else {
            panic!("No architecture set")
        };

        let latency = match std::env::var(arch_mnemonic_str) {
            Ok(latency) => latency.parse::<u32>().unwrap(),
            _ => 1,
        };

        Instruction {
            address: insn.address(),
            mnemonic,
            operands: (
                operands.0.map(|s| s.to_string()),
                operands.1.map(|s| s.to_string()),
            ),
            latency,
        }
    }
}

impl std::fmt::Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let operands_str = match &self.operands {
            (Some(op1), Some(op2)) => format!("{}, {}", op1, op2),
            (Some(op1), None) => op1.to_string(),
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
