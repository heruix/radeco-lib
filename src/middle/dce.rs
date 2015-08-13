//! Dead code elimination

use std::collections::VecDeque;
use petgraph::graph::NodeIndex;
use middle::ssa::{SSAMod, SSA};
use middle::ssa::ssa_traits::NodeType;

/// Removes SSA nodes that are not used by any other node.
/// The algorithm will not consider whether the uses keeping a node alive
/// are in code that is actually executed or not. For a better analysis
/// look at `analysis::constant_propagation`.
pub fn collect<'a, T>(ssa: &mut T) where T:
	SSAMod<ValueRef=NodeIndex, ActionRef=NodeIndex> +
	Clone
{
	let exit_node = ssa.exit_node();
	let roots = ssa.registers_at(exit_node);
	if exit_node == ssa.invalid_value() { panic!(); }
	if roots == ssa.invalid_value() { panic!(); }

	let maxindex = ssa.node_count();
	let mut reachable = Vec::with_capacity(maxindex);
	let mut queue: VecDeque<NodeIndex> = VecDeque::new();
	for i in 0..maxindex {
		reachable.push(ssa.get_node_data(&NodeIndex::new(i)).map(|nd| match nd.nt {
			NodeType::Op(op) => op.has_sideeffects(),
			_                => false,
		}).unwrap_or(true /* If it's not a value, don't delete it. */));
	}
	reachable[roots.index()] = false;
	queue.extend(&[roots]);
	while let Some(ni) = queue.pop_front() {
		let i = ni.index();

		if reachable[i] {
			continue;
		}

		reachable[i] = true;
		queue.extend(ssa.args_of(ni));
	}
	for i in 0..reachable.len() {
		if !reachable[i] {
			ssa.remove(NodeIndex::new(i));
		}
	}
	ssa.cleanup();
}

