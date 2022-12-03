use std::borrow::Cow;

use dot::{Edges, GraphWalk, Id, LabelText, Labeller, Nodes};

use crate::Block;

type Node<'a> = Block<'a>;
type Edge = (u64, u64);

#[derive(Debug, Clone)]
pub struct Graph<'a> {
    pub nodes: Vec<Node<'a>>,
    pub edges: Vec<Edge>,
}

impl<'a> Labeller<'a, Node<'_>, Edge> for Graph<'_> {
    fn graph_id(&'a self) -> Id<'a> {
        Id::new("CFG").unwrap()
    }

    fn node_id(&'a self, n: &Node) -> Id<'a> {
        Id::new(format!("N_0x{:x}", n.leader)).unwrap()
    }

    fn node_label<'b>(&'b self, n: &Node) -> LabelText<'b> {
        let mut label = String::new();

        for insn in &n.insns {
            label.push_str(&format!(
                "0x{:x}: {}\n",
                insn.address(),
                insn.mnemonic().unwrap()
            ));
        }

        LabelText::LabelStr(Cow::Owned(label))
    }

    fn edge_label<'b>(&'b self, e: &Edge) -> LabelText<'b> {
        let edge_label = format!("0x{:x} -> 0x{:x}", e.0, e.1);
        LabelText::LabelStr(Cow::Owned(edge_label))
    }
}

impl<'a> GraphWalk<'a, Node<'a>, Edge> for Graph<'a> {
    fn nodes(&'a self) -> Nodes<'a, Node<'_>> {
        self.nodes.iter().map(|s| s.to_owned()).collect()
    }

    fn edges(&'a self) -> Edges<'a, Edge> {
        self.edges.iter().map(|s| s.to_owned()).collect()
    }

    fn source(&'a self, edge: &Edge) -> Node<'a> {
        let source = self.nodes.iter().find(|n| n.leader == edge.0).unwrap();
        source.to_owned()
    }

    fn target(&'a self, edge: &Edge) -> Node<'a> {
        let target = self.nodes.iter().find(|n| n.leader == edge.1).unwrap();
        target.to_owned()
    }
}
