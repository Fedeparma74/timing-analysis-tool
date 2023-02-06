# Timing analysis tool

Command-line tool for the analysis of an object file to recreate the Control Flow
Graph (CFG) and to annotate it with architectural timing information, estimating
the worst-case execution time (WCET) of the program.

## Architecture support

This tool has been programmed in Rust and
it supports the following architectures: Intelx86, ARM, RISC-V, SPARC, MIPS, and
PowerPC (64-bit and 32-bit modes).

## How it works

After launching the tool with the path of the desired object file, it will print
the WCET of the program on the shell and it will create a */graphs* subdirectory
and a *graph.dot* file inside it, which is the CFG of the program, annotated with
the timing information of each edge. In this subdirectory, also other *dot* files
are generated to better visualize the condensed graph and the reconstructed graphs
of the cycles. The graphs can be visualized using the Graphviz tool, which is a
graph visualization software. A series of ambient variables can also be set by the
user to set specific timing information for an instruction type or to set a specific
maximum number of iterations for a loop or recursive function. Moreover, different
warnings could be printed on the shell, to inform the user about possible
approximations made in the WCET calculation process, such as the presence of external
calls, indirect jumps, recursive functions, or loops with multiple exit points.

In the case of recursive functions, the tool will create also a parallel graph in the
*graph.dot* file that represents the CFG of the recursive function, which is the subgraph
of the original graph. Due to the single return instruction, this graph is unconnected
from the principal graph and it contains a loop, that we called the "return" loop, whose
entry block is the one containing the return instruction. 
This "return" loop is executed at the end of all recursive invocations to
finalize the execution of the function, through the different allocated stacks.

In case of multiple exit points in a loop or if the exit block of the loop is different
from the entry block, the WCET of the loop is calculated in the following way: if the
entry block is not one of the exit blocks, the more distant exit block is chosen
as the exit block of the loop; otherwise, the entry block is chosen also as exit block.
This is done to always consider the pessimistic case. Eventually, in case of multiple
entry points in a loop, the entry point is chosen to be equal to the exit point, to
consider a normal cycle condition. A warning is generated in these cases to inform the
user about the taken choices.

## How to use

To build the tool from source, you need to install the Rust toolchain. You can
find the instructions on the official website: https://www.rust-lang.org/tools/install.
Build the tool with the following command:

```bash
cargo build --release
```

The executable will be located in the *target/release* directory. To run the tool,
you need to specify the path of the object file you want to analyze. For example,
if you want to analyze the *fibonacci_INTELX86_64.o* object file in the examples,
you can run the following command:

```bash
./target/release/timing-analysis-tool ./examples/fibonacci_INTELX86_64.o
```