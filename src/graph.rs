#![allow(dead_code)]
use std::collections::{hash_map, HashMap};

use petgraph::algo::{bellman_ford, condensation};
use petgraph::dot::Dot;
use petgraph::stable_graph::EdgeIndex;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::io::Write;

use crate::block::Block;
use crate::MAX_CYCLES;

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
        println!("leader: {:x}", &source.leader);

        let min_path_latency = paths
            .distances
            .iter()
            .filter(|x| x.is_finite())
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap()
            .to_owned();

        //print distances
        // for (i, distance) in paths.distances.iter().enumerate() {
        //     if distance.is_finite() {
        //         let node = graph.node_weight(NodeIndex::new(i)).unwrap();
        //         println!("distance: {:?}, leader: {:x}", distance, &node.leader);
        //     }
        // }

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

    pub fn reconstruct_longest_path(
        &mut self,
        source: &Block,
        entry_block: &Block,
        exit_block: &Block,
        entry_node_latency: f32,
    ) -> Result<f32, petgraph::algo::NegativeCycle> {
        if entry_block.leader == exit_block.leader {
            let cycle_path = self.longest_path(&source).unwrap() as f32 + entry_node_latency;
            let directed_path = cycle_path - self.longest_path(exit_block).unwrap() as f32;
            let total_cyle_path = cycle_path * MAX_CYCLES as f32 + directed_path; //TO DO NUMVER OF CYCLES
            return Ok(total_cyle_path);
        } else {
            match self.longest_path(&source) {
                Ok(_x) => {}
                Err(e) => {
                    return Err(e);
                }
            }

            //find last block in the graph
            let mut last_block = source.clone();
            let mut last_block_neighbors = self.neighbors_directed(&source, Direction::Outgoing);
            while last_block_neighbors.len() > 0 {
                last_block = last_block_neighbors[0].clone();
                last_block_neighbors = self.neighbors_directed(&last_block, Direction::Outgoing);
            }

            //cloning of the original graph
            let mut graph_initial = self.graph.clone();
            let initial_map = self.node_index_map.clone();

            println!(
                "source: {:x} {:x}",
                &source.leader,
                &last_block.get_call_next_target().unwrap()
            );

            let edge_index =
                self.edge_index_map[&(source.leader, last_block.get_call_next_target().unwrap())];
            self.graph.remove_edge(edge_index);
            let directed_path = self.longest_path(&source).unwrap() as f32 + entry_node_latency
                - self.longest_path(exit_block).unwrap() as f32;

            let digraph = self.to_dot_graph();
            let mut dot_file = std::fs::File::create(format!("reconstrcuted_graph.dot",))
                .expect("Unable to create file");
            dot_file
                .write_all(digraph.as_bytes())
                .expect("Unable to write dot file");

            println!("directed path: {}", directed_path);

            //remove edges outgoing source block exepect those incoming to exit_block.get_targets()[1]
            let mut edges_to_remove = Vec::new();
            for edge in
                graph_initial.edges_directed(initial_map[&source.leader], Direction::Outgoing)
            {
                if edge.target() != initial_map[&last_block.get_call_next_target().unwrap()] {
                    edges_to_remove.push(edge.id());
                }
            }

            for edge in edges_to_remove {
                graph_initial.remove_edge(edge);
            }

            for edge in graph_initial.edge_weights_mut() {
                *edge = -*edge;
            }

            let paths = bellman_ford(&graph_initial, initial_map[&source.leader])?;
            println!("leader: {:x}", &source.leader);

            let min_path_latency = paths
                .distances
                .iter()
                .filter(|x| x.is_finite())
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap()
                .to_owned();

            let cycle_path = min_path_latency * -1.0 + entry_node_latency;

            // let cycle_path = self.longest_path(&source).unwrap() as f32 + entry_node_latency;
            let total_cyle_path = cycle_path * MAX_CYCLES as f32 + directed_path; //TO DO NUMVER OF CYCLES

            println!("cycle path: {}", cycle_path);

            //print dot graph of graph_initial
            let digraph = Dot::with_config(&graph_initial, &[]);
            let mut dot_file = std::fs::File::create(format!("reconstrcuted_graph.dot",))
                .expect("Unable to create file");
            dot_file
                .write_all(digraph.to_string().as_bytes())
                .expect("Unable to write dot file");

            

            return Ok(total_cyle_path as f32);
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
        //print exadecimal leader

        println!("{:x}", &source[0].leader);
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

    pub fn reconstruct_longest_path(
        &mut self,
        source: &[Block],
        entry_block: &Block,
        exit_block: &[Block],
        last_block: &[Block],
        entry_node_latency: f32,
    ) -> Result<f32, petgraph::algo::NegativeCycle> {
        if entry_block.leader == exit_block[0].leader {
            let cycle_path = self.longest_path(&source).unwrap() as f32 + entry_node_latency;
            let directed_path = cycle_path - self.longest_path(exit_block).unwrap() as f32;
            let total_cyle_path = cycle_path * MAX_CYCLES as f32 + directed_path; //TO DO NUMVER OF CYCLES
            return Ok(total_cyle_path);
        } else {
            match self.longest_path(source) {
                Ok(x) => {}
                Err(e) => {
                    return Err(e);
                }
            }
            //cloning of the original graph
            let mut graph_initial = self.graph.clone();
            let initial_map = self.node_index_map.clone();

            let edge_index = self.edge_index_map[&(
                source[0].leader,
                last_block[0].get_call_next_target().unwrap(),
            )];
            self.graph.remove_edge(edge_index);
            let directed_path = self.longest_path(source).unwrap() as f32 + entry_node_latency
                - self.longest_path(exit_block).unwrap() as f32;

            //remove edges outgoing source block except those incoming to exit_block.get_targets()[1]
            let mut edges_to_remove = Vec::new();
            for edge in
                graph_initial.edges_directed(initial_map[&source[0].leader], Direction::Outgoing)
            {
                if edge.target() != initial_map[&last_block[0].get_call_next_target().unwrap()] {
                    edges_to_remove.push(edge.id());
                }
            }

            println!(
                "source: {:x} {:x}",
                &source[0].leader,
                &last_block[0].get_call_next_target().unwrap()
            );

            for edge in edges_to_remove {
                graph_initial.remove_edge(edge);
            }

            for edge in graph_initial.edge_weights_mut() {
                *edge = -*edge;
            }

            let paths = bellman_ford(&graph_initial, initial_map[&source[0].leader])?;
            println!("leader: {:x}", &source[0].leader);

            let min_path_latency = paths
                .distances
                .iter()
                .filter(|x| x.is_finite())
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap()
                .to_owned();

            let cycle_path = min_path_latency * -1.0 + entry_node_latency;

            // let cycle_path = graph_initial.longest_path(source).unwrap() as f32 + entry_node_latency;
            let total_cyle_path = cycle_path + directed_path; //TO DO NUMBER OF CYCLES

            let digraph = self.to_dot_graph();
            let mut dot_file = std::fs::File::create(format!("reconstrcuted_graph.dot",))
                .expect("Unable to create file");
            dot_file
                .write_all(digraph.as_bytes())
                .expect("Unable to write dot file");

            println!("cycle_path: {}", cycle_path);
            println!("directed_path: {}", directed_path);

            return Ok(total_cyle_path as f32);
        }
    }

    pub fn to_dot_graph(&self) -> String {
        let digraph = Dot::with_config(&self.graph, &[]);
        format!("{:?}", digraph)
    }
}
