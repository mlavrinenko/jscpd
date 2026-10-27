// classes.rs — clone classes: the pairs of one duplicated block joined into
// one entry that lists every copy (kucherenko/jscpd#1000).
//
// The detector reports pairs, so a block pasted three times comes out as two
// or three of them. A class joins the pairs that share a copy, a copy being a
// file and the lines it spans, so each copy appears once. A copy that starts
// on the same line as another but ends elsewhere is another copy: joining on
// the start line alone chains the sliding windows of short clones into one
// class of unrelated snippets.

use crate::models::CpdClone;
use std::collections::HashMap;

/// One copy of a class: a file and the lines it spans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassFragment {
    pub source_id: String,
    pub start: u32,
    pub end: u32,
}

/// The pairs that share fragments, joined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneClass {
    /// Every copy once, sorted by file, then by start and end line.
    pub fragments: Vec<ClassFragment>,
    /// Index of the pair with the most tokens, the first one on a tie. Its
    /// text, kind and format stand for the class.
    pub representative: usize,
    /// The representative's token count.
    pub tokens: u32,
    /// True when any pair of the class is new to the baseline.
    pub is_new: bool,
}

/// Join `clones` into classes, sorted by tokens, most first, then by the
/// first fragment's file and line.
pub fn group(clones: &[CpdClone]) -> Vec<CloneClass> {
    let mut nodes = Nodes::default();
    for clone in clones {
        let a = nodes.add(&clone.fragment_a);
        let b = nodes.add(&clone.fragment_b);
        nodes.union(a, b);
    }
    let mut by_root: HashMap<usize, CloneClass> = HashMap::new();
    for (index, clone) in clones.iter().enumerate() {
        let root = nodes.find(nodes.index[&copy(&clone.fragment_a)]);
        let class = by_root.entry(root).or_insert_with(|| CloneClass {
            fragments: Vec::new(),
            representative: index,
            tokens: clone.token_count,
            is_new: false,
        });
        if clone.token_count > class.tokens {
            class.representative = index;
            class.tokens = clone.token_count;
        }
        class.is_new |= clone.is_new;
    }
    for node in 0..nodes.fragments.len() {
        let root = nodes.find(node);
        if let Some(class) = by_root.get_mut(&root) {
            class.fragments.push(nodes.fragments[node].clone());
        }
    }
    let mut classes: Vec<CloneClass> = by_root.into_values().collect();
    for class in &mut classes {
        class
            .fragments
            .sort_by(|x, y| (&x.source_id, x.start, x.end).cmp(&(&y.source_id, y.start, y.end)));
    }
    classes.sort_by(|x, y| {
        let first = |c: &CloneClass| {
            c.fragments
                .first()
                .map(|f| (f.source_id.clone(), f.start, f.end))
        };
        y.tokens
            .cmp(&x.tokens)
            .then_with(|| first(x).cmp(&first(y)))
    });
    classes
}

fn copy(fragment: &crate::models::Fragment) -> ClassFragment {
    ClassFragment {
        source_id: fragment.source_id.clone(),
        start: fragment.start.line,
        end: fragment.end.line,
    }
}

/// A union-find over copies.
#[derive(Default)]
struct Nodes {
    index: HashMap<ClassFragment, usize>,
    fragments: Vec<ClassFragment>,
    parent: Vec<usize>,
}

impl Nodes {
    fn add(&mut self, fragment: &crate::models::Fragment) -> usize {
        let copy = copy(fragment);
        if let Some(&node) = self.index.get(&copy) {
            return node;
        }
        let node = self.fragments.len();
        self.index.insert(copy.clone(), node);
        self.fragments.push(copy);
        self.parent.push(node);
        node
    }

    /// The root of `node`, halving the path on the way.
    fn find(&mut self, mut node: usize) -> usize {
        while self.parent[node] != node {
            self.parent[node] = self.parent[self.parent[node]];
            node = self.parent[node];
        }
        node
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        // The older root wins, so a class's root is its first fragment.
        let (keep, drop) = if ra <= rb { (ra, rb) } else { (rb, ra) };
        self.parent[drop] = keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Fragment, Location};

    fn pair(a: (&str, u32, u32), b: (&str, u32, u32), tokens: u32) -> CpdClone {
        let fragment = |(id, start, end): (&str, u32, u32)| {
            Fragment::new(
                id,
                Location::new(start, 0, 0),
                Location::new(end, 0, 0),
                [0, 0],
            )
        };
        CpdClone::exact("rust", fragment(a), fragment(b), tokens)
    }

    fn spans(class: &CloneClass) -> Vec<(&str, u32, u32)> {
        class
            .fragments
            .iter()
            .map(|f| (f.source_id.as_str(), f.start, f.end))
            .collect()
    }

    #[test]
    fn no_pairs_make_no_classes() {
        assert!(group(&[]).is_empty());
    }

    #[test]
    fn one_pair_is_a_class_of_two() {
        let classes = group(&[pair(("b.rs", 5, 9), ("a.rs", 1, 5), 60)]);
        assert_eq!(classes.len(), 1);
        assert_eq!(spans(&classes[0]), [("a.rs", 1, 5), ("b.rs", 5, 9)]);
        assert_eq!(classes[0].tokens, 60);
        assert_eq!(classes[0].representative, 0);
    }

    #[test]
    fn a_star_of_pairs_on_one_copy_is_one_class() {
        let classes = group(&[
            pair(("a.rs", 1, 5), ("b.rs", 1, 5), 60),
            pair(("a.rs", 1, 5), ("c.rs", 3, 7), 60),
            pair(("a.rs", 1, 5), ("d.rs", 9, 13), 60),
        ]);
        assert_eq!(classes.len(), 1);
        assert_eq!(
            spans(&classes[0]),
            [
                ("a.rs", 1, 5),
                ("b.rs", 1, 5),
                ("c.rs", 3, 7),
                ("d.rs", 9, 13)
            ]
        );
    }

    #[test]
    fn a_chain_of_pairs_joins_through_the_copy_they_share() {
        let classes = group(&[
            pair(("a.rs", 1, 5), ("b.rs", 1, 5), 60),
            pair(("c.rs", 1, 5), ("d.rs", 1, 5), 60),
            pair(("b.rs", 1, 5), ("c.rs", 1, 5), 60),
        ]);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].fragments.len(), 4);
    }

    #[test]
    fn pairs_with_no_copy_in_common_stay_apart() {
        let classes = group(&[
            pair(("a.rs", 1, 5), ("b.rs", 1, 5), 60),
            pair(("a.rs", 2, 6), ("c.rs", 1, 5), 60),
        ]);
        assert_eq!(classes.len(), 2);
    }

    #[test]
    fn copies_that_start_on_one_line_but_end_apart_stay_apart() {
        let classes = group(&[
            pair(("a.rs", 1, 5), ("b.rs", 1, 5), 60),
            pair(("a.rs", 1, 8), ("c.rs", 1, 8), 90),
        ]);
        assert_eq!(classes.len(), 2);
        assert_eq!(spans(&classes[0]), [("a.rs", 1, 8), ("c.rs", 1, 8)]);
        assert_eq!(spans(&classes[1]), [("a.rs", 1, 5), ("b.rs", 1, 5)]);
    }

    #[test]
    fn the_pair_with_most_tokens_stands_for_the_class() {
        let classes = group(&[
            pair(("a.rs", 1, 8), ("b.rs", 1, 8), 60),
            pair(("a.rs", 1, 8), ("c.rs", 1, 8), 90),
            pair(("a.rs", 1, 8), ("d.rs", 1, 8), 90),
        ]);
        assert_eq!(classes[0].representative, 1);
        assert_eq!(classes[0].tokens, 90);
    }

    #[test]
    fn a_class_is_new_when_any_of_its_pairs_is() {
        let mut new = pair(("a.rs", 1, 5), ("c.rs", 1, 5), 60);
        new.is_new = true;
        let classes = group(&[pair(("a.rs", 1, 5), ("b.rs", 1, 5), 60), new]);
        assert!(classes[0].is_new);
        let old = group(&[pair(("a.rs", 1, 5), ("b.rs", 1, 5), 60)]);
        assert!(!old[0].is_new);
    }

    #[test]
    fn classes_sort_by_tokens_then_by_their_first_copy() {
        let classes = group(&[
            pair(("c.rs", 1, 5), ("d.rs", 1, 5), 60),
            pair(("a.rs", 1, 5), ("b.rs", 1, 5), 60),
            pair(("e.rs", 1, 5), ("f.rs", 1, 5), 70),
            pair(("a.rs", 7, 9), ("f.rs", 7, 9), 60),
        ]);
        let firsts: Vec<_> = classes
            .iter()
            .map(|c| (c.fragments[0].source_id.as_str(), c.fragments[0].start))
            .collect();
        assert_eq!(firsts, [("e.rs", 1), ("a.rs", 1), ("a.rs", 7), ("c.rs", 1)]);
    }
}
