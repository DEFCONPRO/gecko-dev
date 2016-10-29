/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use atomic_refcell::AtomicRefCell;
use context::{LocalStyleContext, SharedStyleContext, StyleContext};
use data::ElementData;
use dom::{NodeInfo, OpaqueNode, StylingMode, TElement, TNode};
use gecko::context::StandaloneStyleContext;
use gecko::wrapper::{GeckoElement, GeckoNode};
use std::mem;
use traversal::{DomTraversalContext, recalc_style_at};
use traversal::RestyleResult;

pub struct RecalcStyleOnly<'lc> {
    context: StandaloneStyleContext<'lc>,
    root: OpaqueNode,
}

impl<'lc, 'ln> DomTraversalContext<GeckoNode<'ln>> for RecalcStyleOnly<'lc> {
    type SharedContext = SharedStyleContext;
    #[allow(unsafe_code)]
    fn new<'a>(shared: &'a Self::SharedContext, root: OpaqueNode) -> Self {
        // See the comment in RecalcStyleAndConstructFlows::new for an explanation of why this is
        // necessary.
        let shared_lc: &'lc Self::SharedContext = unsafe { mem::transmute(shared) };
        RecalcStyleOnly {
            context: StandaloneStyleContext::new(shared_lc),
            root: root,
        }
    }

    fn process_preorder(&self, node: GeckoNode<'ln>) -> RestyleResult {
        if node.is_text_node() {
            // Text nodes don't have children, so save the traversal algorithm
            // the trouble of iterating the children.
            RestyleResult::Stop
        } else {
            let el = node.as_element().unwrap();
            recalc_style_at::<_, _, Self>(&self.context, self.root, el)
        }
    }

    fn process_postorder(&self, _: GeckoNode<'ln>) {
        unreachable!();
    }

    /// We don't use the post-order traversal for anything.
    fn needs_postorder_traversal(&self) -> bool { false }

    fn should_traverse_child(_parent: GeckoElement<'ln>, child: GeckoNode<'ln>) -> bool {
        match child.as_element() {
            Some(el) => el.styling_mode() != StylingMode::Stop,
            None => false, // Gecko restyle doesn't need to traverse text nodes.
        }
    }

    fn ensure_element_data<'a>(element: &'a GeckoElement<'ln>) -> &'a AtomicRefCell<ElementData> {
        element.ensure_data()
    }

    fn local_context(&self) -> &LocalStyleContext {
        self.context.local_context()
    }
}
