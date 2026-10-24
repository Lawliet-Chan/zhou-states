use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::inventory::Inventory;

/// 所有实体共用一个递增的 id 空间。
pub type Id = u32;

/// 整个游戏世界的状态。所有容器都用 BTreeMap，保证遍历顺序固定、结算结果可复现。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameState {
    /// 下一个要结算的回合，从 0 开始
    pub turn: u32,
    pub next_id: Id,
    pub characters: BTreeMap<Id, Character>,
    pub cities: BTreeMap<Id, City>,
}

impl GameState {
    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn season_index(&self, config: &Config) -> usize {
        self.turn as usize % config.seasons.len()
    }

    pub fn year(&self, config: &Config) -> u32 {
        self.turn / config.seasons.len() as u32 + 1
    }
}

/// 人物：玩家，或者出仕成为人物的士。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: Id,
    pub name: String,
    /// 爵位：tianzi / gong / hou / bo / zi / nan / none
    pub rank: String,
    pub reputation: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CityStatus {
    /// 所有权和管理权都不属于任何人
    Free,
    /// 被攻占
    Occupied,
    /// 被统治
    Ruled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct City {
    pub id: Id,
    pub name: String,
    pub owner: Option<Id>,
    pub manager: Option<Id>,
    pub status: CityStatus,
    pub totem: String,
    /// 城市拥有的自然资源，比如 farmland、salt_pond
    pub resources: BTreeSet<String>,
    /// 建筑 id → 数量
    pub buildings: BTreeMap<String, u32>,
    pub treasury: Inventory,
    /// 民间财富：民营产出归全城民众共有，城内各群人之间自动互通
    pub private_wealth: Inventory,
    pub tax_rate: f64,
    pub groups: BTreeMap<Id, PopGroup>,
    pub officials: BTreeMap<Id, Official>,
    pub assignments: BTreeMap<Id, Assignment>,
}

impl City {
    /// 所有者、管理者、城市状态带来的产能倍率。
    pub fn efficiency(&self, config: &Config) -> f64 {
        let mut mult = 1.0;
        if self.owner != self.manager {
            mult *= config.economy.absentee_mult;
        }
        if self.status == CityStatus::Occupied {
            mult *= config.economy.occupied_mult;
        }
        mult
    }

    pub fn has_building(&self, building: &str) -> bool {
        self.buildings.get(building).is_some_and(|n| *n > 0)
    }

    /// 某群人中被工事占用的劳力（生产和营建）。
    pub fn labor_assigned(&self, group: Id) -> u32 {
        self.assignments
            .values()
            .filter(|a| a.task.uses_labor())
            .filter_map(|a| a.workers.get(&group))
            .sum()
    }

    /// 某群人中被收税工事覆盖的人数。
    pub fn tax_covered(&self, group: Id) -> u32 {
        self.assignments
            .values()
            .filter(|a| matches!(a.task, Task::CollectTax))
            .filter_map(|a| a.workers.get(&group))
            .sum()
    }

    /// 某群人中未被管理的人数。
    pub fn unmanaged(&self, group: Id) -> u32 {
        let size = self.groups.get(&group).map_or(0, |g| g.size);
        size.saturating_sub(self.labor_assigned(group))
    }

    pub fn assignment_of(&self, official: Id) -> Option<&Assignment> {
        self.assignments.values().find(|a| a.shi == official)
    }

    pub fn population(&self) -> u32 {
        self.groups.values().map(|g| g.size).sum()
    }
}

/// 一群人：同一座城中文化和技能组合相同的人口。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopGroup {
    pub id: Id,
    pub name: String,
    pub size: u32,
    /// 技能槽，顺序有意义：未被管理时按第一项技能自主民营
    pub skills: Vec<String>,
    pub totem: String,
    pub culture: f64,
    pub happiness: f64,
    /// 人口增长的小数部分，攒满 1 人再加入
    pub growth_carry: f64,
}

impl PopGroup {
    pub fn has_skill(&self, skill: &str) -> bool {
        self.skills.iter().any(|s| s == skill)
    }
}

/// 被任用的士人。他已经离开原来的人群，由府库供养。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Official {
    pub id: Id,
    pub name: String,
    pub skills: Vec<String>,
    pub loyalty: f64,
    /// 已任职的回合数，影响管理上限
    pub service_turns: u32,
    /// 原来所属的人群，罢免后回到这里
    pub origin_group: Id,
}

impl Official {
    /// 能管理的人口上限。
    pub fn capacity(&self, config: &Config) -> u32 {
        let shi = &config.shi;
        let experience = if shi.ramp_turns == 0 {
            1.0
        } else {
            (self.service_turns as f64 / shi.ramp_turns as f64).min(1.0)
        };
        let ramp = shi.new_capacity_ratio + (1.0 - shi.new_capacity_ratio) * experience;
        let loyalty = (self.loyalty / shi.loyalty_max).clamp(0.0, 1.0);
        let loyalty_mult = shi.loyalty_min_mult + (1.0 - shi.loyalty_min_mult) * loyalty;
        let capacity = shi.base_capacity * config.specialization(self.skills.len()) * ramp * loyalty_mult;
        capacity.floor() as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// 官营：产出全部归府库
    Guan,
    /// 民营：产出归民众，府库只能通过收税获得
    Min,
}

/// 工事的具体内容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Task {
    Produce { recipe: String, mode: Mode },
    /// 营建一律视为官营
    Construct { building: String },
    CollectTax,
    /// 祭祀只持续一个回合
    Sacrifice,
}

impl Task {
    /// 是否占用投入人口的劳力。收税只是覆盖这些人，不占用他们干活。
    pub fn uses_labor(&self) -> bool {
        matches!(self, Task::Produce { .. } | Task::Construct { .. })
    }
}

/// 一项工事：一名士人主事，带着若干人做一件事。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: Id,
    pub shi: Id,
    pub task: Task,
    /// 人群 id → 人数。收税时表示被覆盖的人数
    pub workers: BTreeMap<Id, u32>,
    /// 营建进度（人 × 回合）
    pub progress: f64,
}

impl Assignment {
    pub fn worker_count(&self) -> u32 {
        self.workers.values().sum()
    }
}
