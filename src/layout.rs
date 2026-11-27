//! Places cards on one grid. Lanes run from left to right toward the merge,
//! so the right lane is ahead. A PR that conflicts with a PR ahead of it sits
//! in a lower row. Both PRs of such a pair are marked, and no other card.
//! A PR that shares files with a placed PR in another lane takes that PR's
//! row when the rule above allows it. Cards are placed front first, so the
//! same board always gives the same layout.

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
    /// PRs that block a PR behind them.
    pub blocking: BTreeSet<u64>,
    /// PRs that conflict with a PR ahead of them.
    pub blocked: BTreeSet<u64>,
    /// Each pull request's place in line, front first.
    pub rank: BTreeMap<u64, usize>,
}

/// Lays out lanes given left to right, each in its own order.
pub fn lay<'a>(lanes: &[Vec<&'a Pull>]) -> Layout<'a> {
    let mut placed: BTreeMap<u64, Spot> = BTreeMap::new();
    let mut blocking = BTreeSet::new();
    let mut blocked = BTreeSet::new();
    let mut next = vec![0; lanes.len()];
    let mut cards = Vec::new();
    let mut rank = BTreeMap::new();
    for (lane, pulls) in lanes.iter().enumerate().rev() {
        for &pull in pulls {
            let mut row = next[lane];
            let mut ahead = Vec::new();
            let mut partners = Vec::new();
            for overlap in &pull.overlaps {
                let Some(spot) = placed.get(&overlap.other) else {
                    continue;
                };
                if overlap.kind == Kind::Conflict {
                    row = row.max(spot.row + 1);
                    ahead.push(overlap.other);
                } else if spot.lane != lane {
                    partners.push(spot.row);
                }
            }
            let mut bound = None;
            for partner in partners {
                if partner >= row && bound.is_none_or(|b| partner < b) {
                    bound = Some(partner);
                }
            }
            row = bound.unwrap_or(row);
            let spot = Spot { lane, row };
            if !ahead.is_empty() {
                blocked.insert(pull.number);
            }
            blocking.extend(ahead);
            next[lane] = row + 1;
            placed.insert(pull.number, spot);
            rank.insert(pull.number, cards.len());
            cards.push((pull, spot));
        }
    }
    Layout {
        cards,
        blocking,
        blocked,
        rank,
    }
}
