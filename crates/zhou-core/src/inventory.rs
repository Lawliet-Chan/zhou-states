use crate::fx::Fx;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 一堆物品：府库、民间财富、配方的投入产出都用它表示。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Inventory(pub BTreeMap<String, Fx>);

impl Inventory {
    pub fn get(&self, good: &str) -> Fx {
        self.0.get(good).copied().unwrap_or(Fx::ZERO)
    }

    pub fn add(&mut self, good: &str, amount: Fx) {
        if amount > Fx::ZERO {
            *self.0.entry(good.to_string()).or_insert(Fx::ZERO) += amount;
        }
    }

    /// 最多取出 `amount`，返回实际取出的数量。
    pub fn take(&mut self, good: &str, amount: Fx) -> Fx {
        let Some(have) = self.0.get_mut(good) else {
            return Fx::ZERO;
        };
        let taken = (*have).min(amount).max(Fx::ZERO);
        *have -= taken;
        if *have <= Fx::ZERO {
            self.0.remove(good);
        }
        taken
    }

    pub fn add_all(&mut self, other: &Inventory) {
        for (good, amount) in &other.0 {
            self.add(good, *amount);
        }
    }

    pub fn contains_all(&self, other: &Inventory) -> bool {
        other
            .0
            .iter()
            .all(|(good, amount)| self.get(good) >= *amount)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Fx)> {
        self.0.iter()
    }
}
