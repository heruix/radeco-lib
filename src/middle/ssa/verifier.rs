//! Implements a pass that goes over the ssa and checks if the ssa is still valid.
//!
//! This is only for verification and to catch potential mistakes.
#![allow(unused_imports, unused_variables)]
use petgraph::graph::{NodeIndex};

use super::cfg_traits::{CFG};
use super::ssa_traits::{SSA, SSAMod, ValueType};
use super::ssastorage::{EdgeData};
use super::ssastorage::NodeData;
use super::ssa_traits::NodeData as TNodeData;
use super::error::SSAErr;

use super::ssastorage::SSAStorage;
use middle::ir::{MArity, MOpcode};
use std::result;
use std::fmt::Debug;

pub type VResult<T> = result::Result<(), SSAErr<T>>;

pub trait Verify: SSA {
	fn verify_block(&self, i: &Self::ActionRef) -> VResult<Self>;
	fn verify_expr(&self, i: &Self::ValueRef) -> VResult<Self>;
}

pub trait VerifiedAdd: SSAMod {
	fn verified_add_op(&mut self, block: Self::ActionRef, opc: MOpcode, vt: ValueType, args: &[Self::ValueRef]) -> Self::ValueRef;
}

impl<T: Verify + SSAMod + Debug> VerifiedAdd for T {
	fn verified_add_op(&mut self, block: Self::ActionRef, opc: MOpcode, vt: ValueType, args: &[Self::ValueRef]) -> Self::ValueRef {
		assert!(opc.allowed_in_ssa());
		let op = self.add_op(block, opc, vt);
		for (i, arg) in args.iter().enumerate() {
			self.op_use(op, i as u8, *arg);
		}
		self.verify_expr(&op).unwrap();
		op
	}
}

macro_rules! check {
	($cond: expr, $ssaerr: expr) => (
		if !$cond { return Err($ssaerr) }
	);
}

impl Verify for SSAStorage {
	fn verify_block(&self, block: &NodeIndex) -> VResult<Self> {
		let edge_count = self.edge_count();
		let node_count = self.node_count();
		// Make sure that we have a valid node first.
		assert!(block.index() < node_count);
		let edges = self.edges_of(block);
		// Every BB can have a maximum of 2 Outgoing CFG Edges.
		check!(edges.len() < 3, SSAErr::WrongNumEdges(*block, 3, edges.len()));

		let mut edgecases = [false; 256];

		for edge in edges.iter() {
			match self.g[*edge] {
				EdgeData::Control(i) => {
					let target = self.target_of(edge);
					check!(self.is_action(target), SSAErr::InvalidType("Block".to_owned()));
					check!(!edgecases[i as usize], SSAErr::InvalidControl(*block, *edge));
					edgecases[i as usize] = true;
				},
				_ => ()
			}
		}

		for edge in edges.iter() {
			assert!(edge.index() < edge_count);
			match self.g[*edge] {
				EdgeData::Control(i) if i < 2 => {
					// Things to lookout for:
					//  * There must be a minimum of two edges.
					//  * There _must_ be a selector.
					//  * The jump targets must not be the same block.
					check!(edges.len() == 2, SSAErr::WrongNumEdges(*block, 2, edges.len()));
					let other_edge = match i {
						0 => self.true_edge_of(block),
						1 => self.false_edge_of(block),
						_ => unreachable!(),
					};
					let target_1 = self.target_of(edge);
					let target_2 = self.target_of(&other_edge);
					check!(target_1 != target_2, SSAErr::InvalidControl(*block, *edge));
					// No need to test the next edge.
					break;
				},
				EdgeData::Control(2) => {
					// Things to lookout for:
					//  * There can be only one Unconditional Edge.
					//  * There can be no selector.
					//  * Make sure we have not introduced an unconditional jump
					//    which self-loops.
					let target_block = self.target_of(edge);
					check!(edges.len() == 1, SSAErr::WrongNumEdges(*block, 1, edges.len()));

					check!(target_block.index() < node_count, SSAErr::InvalidTarget(*block, *edge, target_block));
					let valid_block = if let NodeData::BasicBlock(_) = self.g[target_block] {
						true
					} else {
						false
					};
					//check!(ri, valid_block);
					check!(*block != target_block, SSAErr::InvalidTarget(*block, *edge, target_block));
				},
				_ => panic!("Found something other than a control edge!"),
			}
		}


		let selector = self.selector_of(block);
		if edges.len() == 2 {
			check!(selector.is_some(), SSAErr::NoSelector(*block));
		} else {
			check!(selector.is_none(), SSAErr::UnexpectedSelector(*block, selector.unwrap()));
		}

		// Make sure that this block is reachable.
		let incoming = self.incoming_edges(block);
		check!((incoming.len() > 0) || *block == self.start_node(), SSAErr::UnreachableBlock(*block));
		Ok(())
	}

	fn verify_expr(&self, i: &NodeIndex) -> VResult<Self> {
		let node_count = self.node_count();
		// Make sure we have a valid node first.
		assert!(i.index() < node_count);
		let node_data = &self.g[*i];

		match *node_data {
			NodeData::Op(opcode, ValueType::Integer { width: w }) => {
				let operands = self.get_operands(i);
				let op_len = operands.len();
				let extract = |x: TNodeData| -> u16 {
					match x.vt {
						ValueType::Integer { width: w } => w,
						/*_ => {
							let panic_str = format!("Found {:?}, expected ValueType::Integer", x);
							panic!(panic_str);
						}*/
					}
				};

				let n = match opcode.arity() {
					MArity::Zero => 0,
					MArity::Unary => 1,
					MArity::Binary => 2,
					_ => unimplemented!(),
				};

				{
					check!(op_len == n, SSAErr::WrongNumOperands(*i, n, op_len));
				}

				if n == 0 { return Ok(()) }
				match opcode {
					MOpcode::OpNarrow(w0) => {
						let w1 = self.get_node_data(&operands[0])
						             .map(&extract)
						             .unwrap();
						check!(w1 > w0, SSAErr::IncompatibleWidth(*i, w, w0));
					},
				    MOpcode::OpWiden(w0) =>  {
						let w1 = self.get_node_data(&operands[0])
						             .map(&extract)
						             .unwrap();
						check!(w1 < w0, SSAErr::IncompatibleWidth(*i, w, w0));
					},
					MOpcode::OpCmp | MOpcode::OpGt | MOpcode::OpLt | MOpcode::OpLteq | MOpcode::OpGteq => {
						check!(w == 1, SSAErr::IncompatibleWidth(*i, 1, w));
					},
					_ => {
						let w0 = self.get_node_data(&operands[0])
						             .map(&extract)
						             .unwrap();
						check!(w0 == w, SSAErr::IncompatibleWidth(*i, w, w0));
						for op in operands.iter() {
							let w1 = self.get_node_data(op)
							             .map(&extract)
						                 .unwrap();
							check!(w == w1, SSAErr::IncompatibleWidth(*i, w, w1));
						}
					},
				}
			},
			_ => check!(false, SSAErr::InvalidExpr(*i)),
		}
		Ok(())
	}
}

pub fn verify<T>(ssa: &T) -> VResult<T> where T: Verify + Debug {
	let blocks = ssa.blocks();
	for block in blocks.iter() {
		// assert the qualities of the block first.
		ssa.verify_block(block).ok().unwrap();
		// Iterate through each node in the block and assert their properties.
		let exprs = ssa.exprs_in(block);
		for expr in exprs.iter() {
			try!(ssa.verify_expr(expr));
		}
	}
	Ok(())
}
