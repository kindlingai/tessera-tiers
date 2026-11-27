//! Places cards on one grid. Lanes run from left to right toward the merge,
//! so the right lane is ahead. A PR that conflicts with a PR ahead of it sits
//! in a lower row, and a zone covers the PR ahead and the PRs it holds back.

use crate::board::Pull;
use crate::overlap::Kind;
use std::collections::BTreeMap;

/// A cell on the grid, counted from zero.
#[derive(Clone, Copy)]
pub struct Spot {
    pub lane: usize,
    pub row: usize,
}

/// The cells from `first` to `last`, both included.
pub struct Zone {
    pub first: Spot,
    pub last: Spot,
}

pub struct Layout<'a> {
    pub cards: Vec<(&'a Pull, Spot)>,
    pub zones: Vec<Zone>,
    /// Each pull request's place in line, front first.
    pub rank: BTreeMap<u64, usize>,
}

/// Lays out lanes given left to right, each in its own order.
pub fn lay<'a>(lanes: &[Vec<&'a Pull>]) -> Layout<'a> {
    let mut placed: BTreeMap<u64, Spot> = BTreeMap::new();
    let mut held: BTreeMap<u64, Vec<Spot>> = BTreeMap::new();
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
            for number in ahead {
                held.entry(number).or_default().push(spot);
            }
            next[lane] = row + 1;
            placed.insert(pull.number, spot);
            rank.insert(pull.number, cards.len());
            cards.push((pull, spot));
        }
    }
    let mut zones = Vec::new();
    for (number, behind) in held {
        let front = placed[&number];
        let mut first = front;
        let mut last = front;
        for spot in behind {
            first.lane = first.lane.min(spot.lane);
            last.lane = last.lane.max(spot.lane);
            last.row = last.row.max(spot.row);
        }
        zones.push(Zone { first, last });
    }
    Layout { cards, zones, rank }
}
