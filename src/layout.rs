//! Places cards on one grid. Lanes run from left to right toward the merge,
//! so the right lane is ahead. A PR that conflicts with a PR ahead of it sits
//! in a lower row. Both PRs of such a pair are marked, and no other card.

use crate::board::Pull;
use crate::overlap::Kind;
use std::collections::{BTreeMap, BTreeSet};

/// A cell on the grid, counted from zero.
#[derive(Clone, Copy)]
pub struct Spot {
    pub lane: usize,
    pub row: usize,
}

pub struct Layout<'a> {
    pub cards: Vec<(&'a Pull, Spot)>,
    /// Cards that block a PR behind them.
    pub blocking: Vec<Spot>,
    /// Cards that conflict with a PR ahead of them.
    pub blocked: Vec<Spot>,
    /// Each pull request's place in line, front first.
    pub rank: BTreeMap<u64, usize>,
}

/// Lays out lanes given left to right, each in its own order.
pub fn lay<'a>(lanes: &[Vec<&'a Pull>]) -> Layout<'a> {
    let mut placed: BTreeMap<u64, Spot> = BTreeMap::new();
    let mut held = BTreeSet::new();
    let mut blocked = Vec::new();
    let mut next = vec![0; lanes.len()];
    let mut cards = Vec::new();
    let mut rank = BTreeMap::new();
    for (lane, pulls) in lanes.iter().enumerate().rev() {
        for &pull in pulls {
            let mut row = next[lane];
            let mut ahead = Vec::new();
            for overlap in &pull.overlaps {
                let Some(spot) = placed.get(&overlap.other) else {
                    continue;
                };
                if overlap.kind == Kind::Conflict {
                    row = row.max(spot.row + 1);
                    ahead.push(overlap.other);
                }
            }
            let spot = Spot { lane, row };
            if !ahead.is_empty() {
                blocked.push(spot);
            }
            held.extend(ahead);
            next[lane] = row + 1;
            placed.insert(pull.number, spot);
            rank.insert(pull.number, cards.len());
            cards.push((pull, spot));
        }
    }
    let blocking = held.iter().map(|number| placed[number]).collect();
    Layout {
        cards,
        blocking,
        blocked,
        rank,
    }
}
