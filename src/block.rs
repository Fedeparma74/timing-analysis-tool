use crate::instruction::Instruction;
use crate::jump::ExitJump;

#[derive(Default, Clone, Hash, PartialEq, Eq)]
pub struct Block {
    pub leader: u64,
    pub instructions: Vec<Instruction>,
    pub exit_jump: Option<ExitJump>,
}

impl Block {
    pub fn new(instruction: Instruction) -> Self {
        Block {
            leader: instruction.address,
            instructions: vec![instruction],
            exit_jump: None,
        }
    }

    pub fn add_instruction(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }

    pub fn set_exit_jump(&mut self, exit_jump: ExitJump) {
        self.exit_jump = Some(exit_jump);
    }

    pub fn get_targets(&self) -> Vec<u64> {
        let mut targets = vec![];

        if let Some(exit_jump) = &self.exit_jump {
            match exit_jump {
                ExitJump::ConditionalRelative { taken, not_taken } => {
                    targets.push(*taken);
                    targets.push(*not_taken);
                }
                ExitJump::UnconditionalRelative(target) => {
                    targets.push(*target);
                }
                ExitJump::ConditionalAbsolute { taken, not_taken } => {
                    targets.push(*taken);
                    targets.push(*not_taken);
                }
                ExitJump::UnconditionalAbsolute(target) => {
                    targets.push(*target);
                }
                ExitJump::Indirect => {}
                ExitJump::Ret(ret_targets) => {
                    targets.push(*ret_targets);
                }
                ExitJump::Call(target, _) => {
                    targets.push(*target);
                }
                ExitJump::Next(target) => {
                    targets.push(*target);
                }
            }
        }

        targets
    }

    pub fn modify_targets(&mut self, new_target: u64, target: u64) {
        if let Some(exit_jump) = &mut self.clone().exit_jump {
            match exit_jump {
                ExitJump::ConditionalRelative { taken, not_taken } => {
                    if taken == &target {
                        self.set_exit_jump(ExitJump::ConditionalRelative {
                            taken: new_target,
                            not_taken: *not_taken,
                        });
                    } else if not_taken == &target {
                        self.set_exit_jump(ExitJump::ConditionalRelative {
                            taken: *taken,
                            not_taken: new_target,
                        });
                    }
                }
                ExitJump::UnconditionalRelative(_) => {
                    self.set_exit_jump(ExitJump::UnconditionalRelative(new_target));
                }
                ExitJump::ConditionalAbsolute { taken, not_taken } => {
                    if taken == &target {
                        self.set_exit_jump(ExitJump::ConditionalAbsolute {
                            taken: new_target,
                            not_taken: *not_taken,
                        });
                    } else if not_taken == &target {
                        self.set_exit_jump(ExitJump::ConditionalAbsolute {
                            taken: *taken,
                            not_taken: new_target,
                        });
                    }
                }
                ExitJump::UnconditionalAbsolute(_) => {
                    self.set_exit_jump(ExitJump::UnconditionalAbsolute(new_target));
                }
                ExitJump::Indirect => {}
                ExitJump::Ret(_) => {
                    self.set_exit_jump(ExitJump::Ret(new_target));
                }
                ExitJump::Call(_, ret) => {
                    self.set_exit_jump(ExitJump::Call(new_target, *ret));
                }
                ExitJump::Next(_) => {
                    self.set_exit_jump(ExitJump::Next(new_target));
                }
            }
        }
    }

    pub fn get_latency(&self) -> u32 {
        self.instructions.iter().map(|i| i.latency).sum()
    }
}

impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for insn in self.instructions.iter() {
            writeln!(f, "{insn}")?;
        }
        if let Some(exit_jump) = &self.exit_jump {
            writeln!(f, "Exit jump: {exit_jump}")?;
        } else {
            writeln!(f, "Exit jump: None")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for insn in self.instructions.iter() {
            writeln!(f, "{insn}")?;
        }
        if let Some(exit_jump) = &self.exit_jump {
            writeln!(f, "Exit jump: {exit_jump}")?;
        } else {
            writeln!(f, "Exit jump: None")?;
        }
        Ok(())
    }
}
