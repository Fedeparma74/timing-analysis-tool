#[macro_use]

mod arch;
mod block;
mod cycle;
mod graph;
mod instruction;
mod jump;
mod wcet;

use std::cell::RefCell;
use std::io::Write;

use capstone::{Capstone, NO_EXTRA_MODE};
use object::{Object, ObjectSection};

use crate::arch::ArchMode;
use crate::wcet::calculate_wcet;

#[macro_export]
macro_rules! printwarning {
    ($($arg:tt)*) => {
        println!("WARNING: {}", format_args!($($arg)*))
    };
}

thread_local! {
    static CURRENT_ARCH: RefCell<Option<ArchMode>> = RefCell::new(None);
}

const GRAPHS_DIR: &str = "graphs";

fn main() {
    dotenv::dotenv().ok(); // load .env file

    let file_name = std::env::args().nth(1).expect("File name not found"); // read the file name from the command line argument

    let file_bytes = std::fs::read(file_name).expect("File not found!"); //prova_3ret.o --> 219, prova_d --> 229,  prova_without_cycles.o --> 139, 3cicli.o --> 241, parenthesis.o -> 319
    let obj_file = object::File::parse(file_bytes.as_slice()).unwrap(); //prova_2for --> 159, ooribile.o --> 230, peggio --> 266, funzioni.o --> 245, funzioni_1ciclo.o --> 252

    let arch = obj_file.architecture();
    let arch_mode = ArchMode::from(arch);
    CURRENT_ARCH.with(|current_arch| {
        *current_arch.borrow_mut() = Some(arch_mode.clone());
    });

    println!("{arch_mode:?}");

    let mut text_section = Vec::new();
    for section in obj_file.sections() {
        // join all the sections .text in one
        if section.name().unwrap().contains("text") {
            text_section.extend_from_slice(section.data().unwrap());
        }
    }

    let mut cs = Capstone::new_raw(arch_mode.arch, arch_mode.mode, NO_EXTRA_MODE, None)
        .expect("Failed to create Capstone handle");
    cs.set_detail(true).unwrap();
    cs.set_skipdata(false).unwrap();

    let instructions = cs
        .disasm_all(&text_section, 0x1000)
        .expect("Failed to disassemble given code");

    //print all the instrcutions in a file
    let mut file = std::fs::File::create("instructions.txt").unwrap();

    for instruction in instructions.iter() {
        writeln!(file, "{}", instruction).unwrap();
    }

    let wcet = calculate_wcet(&cs, &arch_mode, &instructions);

    println!("WCET: {wcet} clock cycles");
}
