#![allow(dead_code)]
use std::collections::{hash_map, HashMap};

use petgraph::algo::{bellman_ford, condensation};
use petgraph::dot::Dot;
use petgraph::stable_graph::EdgeIndex;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::block::Block;

#[derive(Debug, Clone)]
pub struct MappedGraph {
    pub graph: StableGraph<Block, f32>,
    pub node_index_map: HashMap<u64, NodeIndex<u32>>,
    pub edge_index_map: HashMap<(u64, u64), EdgeIndex<u32>>,
}

impl MappedGraph {
    pub fn new() -> Self {
        MappedGraph {
            graph: StableGraph::new(),
            node_index_map: HashMap::new(),
            edge_index_map: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, block: Block) {
        if let hash_map::Entry::Vacant(e) = self.node_index_map.entry(block.leader) {
            let node_index = self.graph.add_node(block);
            e.insert(node_index);
        }
    }

    pub fn remove_node(&mut self, block: &Block) {
        let node_index = self.node_index_map[&block.leader];
        self.graph.remove_node(node_index);
        self.node_index_map.remove(&block.leader);
    }

    pub fn get_nodes(&self) -> Vec<Block> {
        self.graph.node_weights().cloned().collect::<Vec<Block>>()
    }

    pub fn add_edge(&mut self, source: Block, target: Block, weight: f32) {
        self.add_node(source.clone());
        self.add_node(target.clone());

        if let hash_map::Entry::Vacant(e) =
            self.edge_index_map.entry((source.leader, target.leader))
        {
            let source_index = self.node_index_map[&source.leader];
            let target_index = self.node_index_map[&target.leader];
            let edge_index = self.graph.add_edge(source_index, target_index, weight);
            e.insert(edge_index);
        }
    }

    pub fn remove_edge(&mut self, source: &Block, target: &Block) {
        let edge_index = self.edge_index_map[&(source.leader, target.leader)];
        self.graph.remove_edge(edge_index);

        self.edge_index_map.remove(&(source.leader, target.leader));
    }

    pub fn update_edge(&mut self, a: &Block, b: &Block, weight: f32) {
        let a_index = self.node_index_map[&a.leader];
        let b_index = self.node_index_map[&b.leader];

        self.graph.update_edge(a_index, b_index, weight);
    }

    pub fn get_edges(&self) -> Vec<(Block, Block, f32)> {
        self.graph
            .edge_indices()
            .map(|edge_index| {
                let edge = self.graph.edge_weight(edge_index).unwrap();
                let (source, target) = self.graph.edge_endpoints(edge_index).unwrap();

                let source = self.graph.node_weight(source).unwrap();
                let target = self.graph.node_weight(target).unwrap();

                (source.clone(), target.clone(), *edge)
            })
            .collect::<Vec<(Block, Block, f32)>>()
    }

    pub fn edges_directed(&self, node: &Block, direction: Direction) -> Vec<(Block, Block, f32)> {
        let node_index = self.node_index_map[&node.leader];
        let edges = self.graph.edges_directed(node_index, direction);

        edges
            .map(|edge| {
                let source = self.graph.node_weight(edge.source()).unwrap();
                let target = self.graph.node_weight(edge.target()).unwrap();

                (source.clone(), target.clone(), *edge.weight())
            })
            .collect::<Vec<(Block, Block, f32)>>()
    }

    pub fn neighbors_directed(&self, node: &Block, direction: Direction) -> Vec<Block> {
        let node_index = self.node_index_map[&node.leader];
        let neighbors = self.graph.neighbors_directed(node_index, direction);

        let mut blocks = Vec::new();

        for neighbor in neighbors {
            let block = self.graph.node_weight(neighbor).unwrap();
            blocks.push(block.clone());
        }

        blocks
    }

    pub fn shortest_path(&self, source: &Block) -> f32 {
        let paths = bellman_ford(&self.graph, self.node_index_map[&source.leader]).unwrap();

        let max_path_latency = paths
            .distances
            .iter()
            .filter(|x| x.is_finite())
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap()
            .to_owned();

        max_path_latency
    }

    pub fn longest_path(&self, source: &Block) -> Result<f32, petgraph::algo::NegativeCycle> {
        // change the weights of the edges to negative values to find the longest path
        let mut graph = self.graph.clone();
        for edge in graph.edge_weights_mut() {
            *edge = -*edge;
        }

        let paths = bellman_ford(&graph, self.node_index_map[&source.leader])?;

        let min_path_latency = paths
            .distances
            .iter()
            .filter(|x| x.is_finite())
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap()
            .to_owned();

        Ok(min_path_latency * -1.0)
    }

    pub fn condense_cycles(&mut self) -> MappedCondensedGraph {
        let condensed_graph = condensation(self.graph.clone().into(), true);
        let stable_condensed_graph: StableGraph<Vec<Block>, f32> = condensed_graph.into();

        let mut node_index_map = HashMap::new();
        let mut edge_index_map = HashMap::new();

        for node_index in stable_condensed_graph.node_indices() {
            let blocks = stable_condensed_graph.node_weight(node_index).unwrap();
            node_index_map.insert(blocks[0].leader, node_index);
        }

        for edge_index in stable_condensed_graph.edge_indices() {
            let (source, target) = stable_condensed_graph.edge_endpoints(edge_index).unwrap();

            let source_blocks = stable_condensed_graph.node_weight(source).unwrap();
            let target_blocks = stable_condensed_graph.node_weight(target).unwrap();

            let source_block = source_blocks.first().unwrap();
            let target_block = target_blocks.first().unwrap();

            edge_index_map.insert((source_block.leader, target_block.leader), edge_index);
        }

        MappedCondensedGraph {
            graph: stable_condensed_graph,
            node_index_map,
            edge_index_map,
        }
    }

    pub fn to_dot_graph(&self) -> String {
        let digraph = Dot::with_config(&self.graph, &[]);
        digraph.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct MappedCondensedGraph {
    pub graph: StableGraph<Vec<Block>, f32>,
    pub node_index_map: HashMap<u64, NodeIndex<u32>>,
    pub edge_index_map: HashMap<(u64, u64), EdgeIndex<u32>>,
}

impl MappedCondensedGraph {
    pub fn get_condensed_nodes(&self) -> Vec<Vec<Block>> {
        let mut condensed_nodes = Vec::new();

        for node_index in self.graph.node_indices() {
            let blocks = self.graph.node_weight(node_index).unwrap();
            if blocks.len() > 1 {
                condensed_nodes.push(blocks.clone());
            }
        }
        condensed_nodes
    }

    pub fn add_node(&mut self, blocks: Vec<Block>) {
        if let hash_map::Entry::Vacant(e) = self.node_index_map.entry(blocks[0].leader) {
            let node_index = self.graph.add_node(blocks.clone());
            e.insert(node_index);
        }
    }

    pub fn remove_node(&mut self, blocks: &[Block]) {
        let node_index = self.node_index_map[&blocks[0].leader];
        self.graph.remove_node(node_index);
    }

    pub fn get_nodes(&self) -> Vec<Vec<Block>> {
        let mut nodes = Vec::new();

        for node_index in self.graph.node_indices() {
            let blocks = self.graph.node_weight(node_index).unwrap();
            nodes.push(blocks.clone());
        }

        nodes
    }

    pub fn add_edge(&mut self, source: Vec<Block>, target: Vec<Block>, weight: f32) {
        self.add_node(source.clone());
        self.add_node(target.clone());

        if let hash_map::Entry::Vacant(e) = self
            .edge_index_map
            .entry((source[0].leader, target[0].leader))
        {
            let source_index = self.node_index_map[&source[0].leader];
            let target_index = self.node_index_map[&target[0].leader];

            let edge_index = self.graph.add_edge(source_index, target_index, weight);
            e.insert(edge_index);
        }
    }

    pub fn remove_edge(&mut self, source: &[Block], target: &[Block]) {
        let edge_index = self.edge_index_map[&(source[0].leader, target[0].leader)];
        self.graph.remove_edge(edge_index);
    }

    pub fn update_edge(&mut self, a: &[Block], b: &[Block], weight: f32) {
        let source_index = self.node_index_map[&a[0].leader];
        let target_index = self.node_index_map[&b[0].leader];

        self.graph.update_edge(source_index, target_index, weight);
    }

    pub fn get_edges(&self) -> Vec<(Vec<Block>, Vec<Block>, f32)> {
        let mut edges = Vec::new();

        for edge_index in self.graph.edge_indices() {
            let (source, target) = self.graph.edge_endpoints(edge_index).unwrap();

            let source_blocks = self.graph.node_weight(source).unwrap();
            let target_blocks = self.graph.node_weight(target).unwrap();

            let weight = *self.graph.edge_weight(edge_index).unwrap();

            edges.push((source_blocks.clone(), target_blocks.clone(), weight));
        }

        edges
    }

    pub fn edges_directed(
        &self,
        node: &[Block],
        direction: Direction,
    ) -> Vec<(Vec<Block>, Vec<Block>, f32)> {
        let node_index = self.node_index_map[&node[0].leader];
        let edges = self.graph.edges_directed(node_index, direction);

        let mut blocks = Vec::new();

        for edge in edges {
            let source = self.graph.node_weight(edge.source()).unwrap();
            let target = self.graph.node_weight(edge.target()).unwrap();
            let weight = *edge.weight();
            blocks.push((source.clone(), target.clone(), weight));
        }

        blocks
    }

    pub fn neighbors_directed(&self, node: &[Block], direction: Direction) -> Vec<Vec<Block>> {
        let node_index = self.node_index_map[&node[0].leader];
        let neighbors = self.graph.neighbors_directed(node_index, direction);

        let mut blocks = Vec::new();

        for neighbor in neighbors {
            let block = self.graph.node_weight(neighbor).unwrap();
            blocks.push(block.clone());
        }

        blocks
    }

    pub fn shortest_path(&self, source: &[Block]) -> f32 {
        let paths = bellman_ford(&self.graph, self.node_index_map[&source[0].leader]).unwrap();

        let max_path_latency = paths
            .distances
            .iter()
            .filter(|x| x.is_finite())
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap()
            .to_owned();

        max_path_latency
    }

    pub fn longest_path(&self, source: &[Block]) -> Result<f32, petgraph::algo::NegativeCycle> {
        // change the weights of the edges to negative values to find the longest path
        let mut graph = self.graph.clone();
        for edge in graph.edge_weights_mut() {
            *edge = -*edge;
        }

        let paths = bellman_ford(&graph, self.node_index_map[&source[0].leader])?;

        let min_path_latency = paths
            .distances
            .iter()
            .filter(|x| x.is_finite())
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap()
            .to_owned();

        Ok(min_path_latency * -1.0)
    }

    pub fn to_dot_graph(&self) -> String {
        let digraph = Dot::with_config(&self.graph, &[]);
        format!("{:?}", digraph)
    }
}
