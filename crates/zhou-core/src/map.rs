//! 六边形地图。每一格是一块地，城市的地块就是地图上属于它的格子。
//!
//! 坐标用轴向坐标 (q, r)。剧本里手画地图时按行列书写（奇数行向右缩进半格），加载时换算。

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::Config;

/// 地图上的一格，轴向坐标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hex {
    pub q: i32,
    pub r: i32,
}

/// 六个方向上相邻格子的坐标差。
const DIRECTIONS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

impl Hex {
    pub const fn new(q: i32, r: i32) -> Self {
        Hex { q, r }
    }

    /// 由行列换算：奇数行向右缩进半格。
    pub fn from_offset(col: i32, row: i32) -> Self {
        Hex {
            q: col - (row - (row & 1)) / 2,
            r: row,
        }
    }

    /// 换算回行列。
    pub fn to_offset(self) -> (i32, i32) {
        (self.q + (self.r - (self.r & 1)) / 2, self.r)
    }

    pub fn neighbors(self) -> impl Iterator<Item = Hex> {
        DIRECTIONS
            .iter()
            .map(move |(dq, dr)| Hex::new(self.q + dq, self.r + dr))
    }

    pub fn is_adjacent(self, other: Hex) -> bool {
        self.distance(other) == 1
    }

    /// 两格之间要走几步。
    pub fn distance(self, other: Hex) -> u32 {
        let dq = self.q - other.q;
        let dr = self.r - other.r;
        ((dq.abs() + dr.abs() + (dq + dr).abs()) / 2) as u32
    }

    /// 以自己为中心、半径为 `radius` 的一圈格子，从正东开始逆时针排列。半径为 0 时只有自己。
    pub fn ring(self, radius: u32) -> Vec<Hex> {
        if radius == 0 {
            return vec![self];
        }
        let radius = radius as i32;
        let (dq, dr) = DIRECTIONS[4];
        let mut hex = Hex::new(self.q + dq * radius, self.r + dr * radius);
        let mut ring = Vec::new();
        for (dq, dr) in DIRECTIONS {
            for _ in 0..radius {
                ring.push(hex);
                hex = Hex::new(hex.q + dq, hex.r + dr);
            }
        }
        ring
    }

    /// 由近及远的螺旋：自己，然后一圈一圈向外。
    pub fn spiral(self) -> impl Iterator<Item = Hex> {
        (0..).flat_map(move |radius| self.ring(radius))
    }
}

impl std::fmt::Display for Hex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (col, row) = self.to_offset();
        write!(f, "({col},{row})")
    }
}

/// 地图上的一格。无主的格子是野地，城市可以把相邻的野地开垦进来。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub pos: Hex,
    pub terrain: String,
}

#[derive(Debug, Clone, Default)]
pub struct Map {
    pub cells: BTreeMap<Hex, Cell>,
}

impl Map {
    pub fn get(&self, pos: Hex) -> Option<&Cell> {
        self.cells.get(&pos)
    }

    pub fn insert(&mut self, pos: Hex, terrain: impl Into<String>) {
        self.cells.insert(
            pos,
            Cell {
                pos,
                terrain: terrain.into(),
            },
        );
    }

    /// 从 `from` 到 `to` 耗时最少的路线，不含起点。`cost` 给出走进一格的耗时，None 表示走不进去。
    /// 耗时相同的路线按坐标取第一条，保证结果可复现。走不到时返回 None。
    pub fn shortest_path(
        &self,
        from: Hex,
        to: Hex,
        cost: impl Fn(Hex) -> Option<u32>,
    ) -> Option<Vec<Hex>> {
        if from == to {
            return Some(Vec::new());
        }
        let mut best: BTreeMap<Hex, (u32, Hex)> = BTreeMap::new();
        let mut queue = BinaryHeap::new();
        queue.push(Reverse((0, from)));
        while let Some(Reverse((spent, pos))) = queue.pop() {
            if pos == to {
                break;
            }
            if best.get(&pos).is_some_and(|(c, _)| *c < spent) {
                continue;
            }
            for next in pos.neighbors() {
                if next == from || self.get(next).is_none() {
                    continue;
                }
                let Some(step) = cost(next) else { continue };
                let total = spent + step;
                if best.get(&next).is_none_or(|(c, _)| total < *c) {
                    best.insert(next, (total, pos));
                    queue.push(Reverse((total, next)));
                }
            }
        }
        best.get(&to)?;
        let mut path = vec![to];
        let mut pos = to;
        while let Some((_, prev)) = best.get(&pos) {
            if *prev == from {
                break;
            }
            path.push(*prev);
            pos = *prev;
        }
        path.reverse();
        Some(path)
    }

    /// 军队走进这一格要花的行军力。没有这一格，或者地形过不去（高山、大河），返回 None。
    pub fn move_cost(&self, pos: Hex, config: &Config) -> Option<u32> {
        self.get(pos)
            .and_then(|cell| config.terrains.get(&cell.terrain))
            .and_then(|t| t.move_cost)
    }
}

// 坐标不能做 JSON 的键，序列化成格子的列表。
impl Serialize for Map {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.cells.values())
    }
}

impl<'de> Deserialize<'de> for Map {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let cells = Vec::<Cell>::deserialize(deserializer)?;
        Ok(Map {
            cells: cells.into_iter().map(|c| (c.pos, c)).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_round_trips() {
        for row in -3..4 {
            for col in -3..4 {
                assert_eq!(Hex::from_offset(col, row).to_offset(), (col, row));
            }
        }
    }

    #[test]
    fn neighbors_are_one_step_away() {
        let center = Hex::new(2, -1);
        assert!(center.neighbors().all(|n| center.distance(n) == 1));
        assert_eq!(center.ring(2).len(), 12);
        assert!(center.ring(2).iter().all(|h| center.distance(*h) == 2));
    }

    #[test]
    fn odd_rows_are_shifted_right() {
        // 第 1 行第 0 格和第 0 行的第 0、1 格相邻
        let below = Hex::from_offset(0, 1);
        assert!(below.is_adjacent(Hex::from_offset(0, 0)));
        assert!(below.is_adjacent(Hex::from_offset(1, 0)));
        assert!(!below.is_adjacent(Hex::from_offset(2, 0)));
    }
}
