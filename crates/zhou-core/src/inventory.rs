use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

const EPS: f64 = 1e-9;

/// 一堆物品：府库、民间财富、配方的投入产出都用它表示。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Inventory(pub BTreeMap<String, f64>);

impl Inventory {
    pub fn get(&self, good: &str) -> f64 {
        self.0.get(good).copied().unwrap_or(0.0)
    }

    pub fn add(&mut self, good: &str, amount: f64) {
        if amount > 0.0 {
            *self.0.entry(good.to_string()).or_insert(0.0) += amount;
        }
    }

    /// 最多取出 `amount`，返回实际取出的数量。
    pub fn take(&mut self, good: &str, amount: f64) -> f64 {
        let Some(have) = self.0.get_mut(good) else {
            return 0.0;
        };
        let taken = have.min(amount).max(0.0);
        *have -= taken;
        if *have <= EPS {
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
        other.0.iter().all(|(good, amount)| self.get(good) + EPS >= *amount)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &f64)> {
        self.0.iter()
    }
}
