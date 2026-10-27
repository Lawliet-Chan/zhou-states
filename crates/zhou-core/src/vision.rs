//! 战争迷雾：每个人只能看到自己视野内的东西，视野不共享。
//!
//! 视野来自自己拥有、管理、攻占的城（城的地块再往外几格）和自己统领的军队（所在的格子再往外几格）。
//! 地形是公开的。别人的军队在视野内才看得到，离开视野后只记得最后一次看到的位置和时间；
//! 别人的城在视野内能看到外观（城名、所有者、国家、大致人口、城墙），看不到内情。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::army::Army;
use crate::config::Config;
use crate::map::Hex;
use crate::state::{City, GameState, Id};

/// 一个人记得的别人的军队和城。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Intel {
    #[serde(default)]
    pub armies: BTreeMap<Id, ArmySighting>,
    #[serde(default)]
    pub cities: BTreeMap<Id, CitySighting>,
}

/// 看到的一支别人的军队。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmySighting {
    pub id: Id,
    pub commander: Id,
    pub pos: Hex,
    /// 兵种 → 人数
    pub units: BTreeMap<String, u32>,
    /// 看到时是第几回合
    pub turn: u32,
}

/// 看到的一座别人的城的外观。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CitySighting {
    pub id: Id,
    pub name: String,
    pub seat: Hex,
    pub tiles: Vec<Hex>,
    pub owner: Option<Id>,
    pub occupier: Option<Id>,
    pub country: Option<Id>,
    /// 大致人口：小邑、中邑、大邑
    pub size: String,
    /// 有没有城墙；城墙破了没有
    pub wall: bool,
    pub wall_broken: bool,
    pub turn: u32,
}

impl Army {
    /// 这支军队能看多远：各兵种里看得最远的。
    pub fn sight(&self, config: &Config) -> u32 {
        self.squads
            .iter()
            .map(|s| config.units[&s.unit].vision.unwrap_or(config.vision.army))
            .max()
            .unwrap_or(config.vision.army)
    }

    /// 这支军队看得到的格子。
    pub fn sees(&self, config: &Config, pos: Hex) -> bool {
        self.pos.distance(pos) <= self.sight(config)
    }
}

/// `who` 眼下看得到的格子。
pub fn vision(state: &GameState, config: &Config, who: Id) -> BTreeSet<Hex> {
    let mut sources: Vec<(Hex, u32)> = Vec::new();
    for city in state.cities.values() {
        if city.insiders().contains(&who) {
            sources.extend(city.tiles.values().map(|t| (t.pos, config.vision.city)));
        }
    }
    for army in state.armies.values().filter(|a| a.commander == who) {
        sources.push((army.pos, army.sight(config)));
    }
    if sources.is_empty() {
        return BTreeSet::new();
    }
    state
        .map
        .cells
        .keys()
        .copied()
        .filter(|p| sources.iter().any(|(s, r)| s.distance(*p) <= *r))
        .collect()
}

/// 城的外观。
pub fn glimpse(state: &GameState, config: &Config, city: &City) -> CitySighting {
    let population = city.population();
    let size = config
        .vision
        .tiers
        .iter()
        .rev()
        .find(|t| population >= t.min)
        .map_or_else(String::new, |t| t.name.clone());
    CitySighting {
        id: city.id,
        name: city.name.clone(),
        seat: city.seat,
        tiles: city.tiles.values().map(|t| t.pos).collect(),
        owner: city.owner,
        occupier: city.occupier,
        country: state.country_of(city),
        size,
        wall: city.wall_hp(config).is_some(),
        wall_broken: city.wall_hp(config).is_some() && !city.wall_standing(config),
        turn: state.turn,
    }
}

fn sighting(state: &GameState, army: &Army) -> ArmySighting {
    let mut units = BTreeMap::new();
    for s in &army.squads {
        *units.entry(s.unit.clone()).or_insert(0) += s.count;
    }
    ArmySighting {
        id: army.id,
        commander: army.commander,
        pos: army.pos,
        units,
        turn: state.turn,
    }
}

/// 所有人看一眼：视野里别人的军队、别人的城记下来；记得的军队原来的位置看得到了、却不在那儿了，就忘掉。
pub fn observe(state: &mut GameState, config: &Config) {
    let people: Vec<Id> = state.characters.keys().copied().collect();
    for who in people {
        let seen = vision(state, config, who);
        if seen.is_empty() && !state.intel.contains_key(&who) {
            continue;
        }
        let armies: Vec<ArmySighting> = state
            .armies
            .values()
            .filter(|a| a.commander != who && seen.contains(&a.pos))
            .map(|a| sighting(state, a))
            .collect();
        let cities: Vec<CitySighting> = state
            .cities
            .values()
            .filter(|c| {
                !c.insiders().contains(&who) && c.tiles.values().any(|t| seen.contains(&t.pos))
            })
            .map(|c| glimpse(state, config, c))
            .collect();
        let intel = state.intel.entry(who).or_default();
        intel.armies.retain(|_, s| !seen.contains(&s.pos));
        for s in armies {
            intel.armies.insert(s.id, s);
        }
        for c in cities {
            intel.cities.insert(c.id, c);
        }
    }
}

/// `army` 自己视野里交战中的敌军。
pub(crate) fn enemies_in_sight(state: &GameState, config: &Config, army: &Army) -> BTreeSet<Id> {
    state
        .armies
        .values()
        .filter(|a| state.at_war(army.commander, a.commander) && army.sees(config, a.pos))
        .map(|a| a.id)
        .collect()
}
