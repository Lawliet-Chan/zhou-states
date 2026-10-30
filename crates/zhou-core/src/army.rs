//! 军队：城主从城里会「军武」的人中征发编成，带着府库领出的装备。
//!
//! 当兵的人离开原来的人群和岗位，遣散时回去。军队属于编成它的城，由这座城的所有者统领；
//! 城被攻占时由占领者统领。

use crate::fx::Fx;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::command::CommandError;
use crate::config::Config;
use crate::event::Event;
use crate::inventory::Inventory;
use crate::map::Hex;
use crate::state::{City, CityStatus, GameState, Id, PopGroup};

/// 一支军队。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Army {
    pub id: Id,
    /// 编成这支军队的城，遣散时人回到这里
    pub city: Id,
    /// 统帅：编成时这座城的所有者（城被攻占时是占领者）。城换了主人，军队跟着换统帅
    pub commander: Id,
    /// 所在的格子，编成时在治所
    pub pos: Hex,
    pub squads: Vec<Squad>,
    /// 行军的目的地。这回合走不到，之后的回合开始时自动接着走，到了或者走不下去就清空
    #[serde(default)]
    pub dest: Option<Hex>,
    /// 这回合还剩的行军力，每回合开始时回满
    #[serde(default)]
    pub moves_left: u32,
    /// 这回合已经发起过攻击，每回合只能攻击一次
    #[serde(default)]
    pub attacked: bool,
    /// 进驻了城墙：能还手，但城墙一破就随之覆灭。离开所在的格子就下了城墙
    #[serde(default)]
    pub on_wall: bool,
    /// 这次行军是硬闯：没有通行权的城也穿过去，走进去就是对城主宣战
    #[serde(default)]
    pub force: bool,
    /// 出征时供粮的粮道
    #[serde(default)]
    pub supply: Option<crate::supply::SupplyLine>,
    /// 连续断粮的回合数，供上粮就归零
    #[serde(default)]
    pub hunger: u32,
}

/// 军队里同一群人出身、同一兵种的一队兵。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Squad {
    pub unit: String,
    pub count: u32,
    /// 出身的人群，遣散时回去；这群人还记着他们的技能槽、文化图腾等，原来的人群没了时用来重建
    pub group: Id,
    pub skills: Vec<String>,
    pub totem: String,
    pub culture: Fx,
    pub happiness: Fx,
    pub loyalty: Fx,
    pub hostility: Fx,
    /// 从哪些工事里抽出来的：工事 id → 人数。遣散时工事还在就回去
    #[serde(default)]
    pub posts: BTreeMap<Id, u32>,
    /// 带着的装备，按实际领到的物品记（精品记精品）
    #[serde(default)]
    pub gear: Inventory,
}

impl Squad {
    /// 必需装备的齐备程度，0 到 1。精品也算。
    pub fn equipped(&self, config: &Config) -> Fx {
        let def = &config.units[&self.unit];
        def.gear
            .iter()
            .map(|(good, per)| {
                let need = *per * self.count;
                if need <= Fx::ZERO {
                    return Fx::ONE;
                }
                let fine = config.goods[good].fine.as_deref();
                let have = self.gear.get(good) + fine.map_or(Fx::ZERO, |f| self.gear.get(f));
                (have / need).min(Fx::ONE)
            })
            .fold(Fx::ONE, Fx::min)
    }

    /// 装备带来的战斗力倍率：装备齐全时为 1，完全没有时为 `military.unequipped_mult`。
    pub fn gear_mult(&self, config: &Config) -> Fx {
        let ratio = self.equipped(config);
        ratio + (Fx::ONE - ratio) * config.military.unequipped_mult
    }

    /// 脱产精兵：技能槽只有「军武」一项，由府库供养。
    pub fn is_elite(&self, config: &Config) -> bool {
        is_elite(&self.skills, config)
    }
}

impl Army {
    pub fn size(&self) -> u32 {
        self.squads.iter().map(|s| s.count).sum()
    }

    /// 每回合的行军力：混了几个兵种时按最慢的算。
    pub fn moves(&self, config: &Config) -> u32 {
        self.squads
            .iter()
            .map(|s| config.units[&s.unit].moves)
            .min()
            .unwrap_or(0)
    }

    /// 走进一格地形的耗时：各兵种里最慢的。过不去的地形返回 None。
    pub fn terrain_cost(&self, config: &Config, terrain: &str) -> Option<u32> {
        let base = config.terrains.get(terrain)?.move_cost?;
        self.squads
            .iter()
            .map(|s| {
                config.units[&s.unit]
                    .terrain_cost
                    .get(terrain)
                    .copied()
                    .unwrap_or(base)
            })
            .max()
    }
}

pub fn is_elite(skills: &[String], config: &Config) -> bool {
    skills.len() == 1 && skills[0] == config.military.skill
}

/// 编军队的一项：从哪群人里出多少人、当什么兵、额外带哪些可选装备。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SquadOrder {
    pub group: Id,
    pub unit: String,
    pub count: u32,
    #[serde(default)]
    pub extras: Vec<String>,
}

/// 谁能统领这座城的军队：所有者；城被攻占时是占领者（管理者）。
pub fn can_command(city: &City, who: Id) -> bool {
    match city.status {
        CityStatus::Occupied => city.occupier == Some(who),
        _ => city.owner == Some(who),
    }
}

/// 征发：从城里会「军武」的人中抽人编成一支军队，装备从府库领。没在做工事的人先上，
/// 不够再从工事里抽人，后开的工事先抽。装备不够也能编，只是战斗力打折。
pub fn raise(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    city_id: Id,
    orders: &[SquadOrder],
) -> Result<Vec<Id>, CommandError> {
    let city = state
        .cities
        .get(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
    if !can_command(city, actor) {
        return Err(CommandError::NotOwner(city_id));
    }
    if orders.is_empty() {
        return Err(CommandError::EmptyArmy);
    }
    let mut per_group: BTreeMap<Id, u32> = BTreeMap::new();
    for order in orders {
        let def = config
            .units
            .get(&order.unit)
            .ok_or_else(|| CommandError::NoSuchUnit(order.unit.clone()))?;
        if order.count == 0 {
            return Err(CommandError::ZeroCount);
        }
        for extra in &order.extras {
            if def.optional.get(extra) <= Fx::ZERO {
                return Err(CommandError::NotOptionalGear {
                    unit: def.name.clone(),
                    good: extra.clone(),
                });
            }
        }
        let group = city
            .groups
            .get(&order.group)
            .ok_or(CommandError::NoSuchGroup(order.group))?;
        if !group.has_skill(&config.military.skill) {
            return Err(CommandError::MissingSkill {
                group: order.group,
                skill: config.military.skill.clone(),
            });
        }
        *per_group.entry(order.group).or_insert(0) += order.count;
    }
    for (group, requested) in &per_group {
        // 正在学技能的人学完再说
        let available = city.groups[group]
            .size
            .saturating_sub(city.learning_in(*group));
        if *requested > available {
            return Err(CommandError::NotEnoughNotLearning {
                group: *group,
                available,
                requested: *requested,
            });
        }
    }

    let id = state.alloc_id();
    let city = state.cities.get_mut(&city_id).expect("checked above");
    let mut squads = Vec::new();
    for order in orders {
        let def = &config.units[&order.unit];
        let posts = pull_from_work(city, order.group, order.count);
        let group = city.groups.get_mut(&order.group).expect("checked above");
        group.size -= order.count;
        let mut gear = Inventory::default();
        let carried = def.gear.iter().chain(
            def.optional
                .iter()
                .filter(|(g, _)| order.extras.contains(g)),
        );
        for (good, per) in carried {
            let mut need = *per * order.count;
            let fine = config.goods[good].fine.as_deref();
            for g in std::iter::once(good.as_str()).chain(fine) {
                let taken = city.treasury.take(g, need);
                gear.add(g, taken);
                need -= taken;
            }
        }
        let group = &city.groups[&order.group];
        squads.push(Squad {
            unit: order.unit.clone(),
            count: order.count,
            group: order.group,
            skills: group.skills.clone(),
            totem: group.totem.clone(),
            culture: group.culture,
            happiness: group.happiness,
            loyalty: group.loyalty,
            hostility: group.hostility,
            posts,
            gear,
        });
    }
    city.end_empty_assignments();
    let pos = city.seat;
    let mut army = Army {
        id,
        city: city_id,
        commander: actor,
        pos,
        squads,
        dest: None,
        moves_left: 0,
        attacked: false,
        on_wall: false,
        force: false,
        supply: None,
        hunger: 0,
    };
    army.moves_left = army.moves(config);
    state.armies.insert(id, army);
    Ok(vec![id])
}

/// 从一群人里抽走 `count` 人之前，先让出他们在工事里的位置：没在做工事的人先走，
/// 不够再从工事里抽，后开的先抽。收税只是覆盖，人少了跟着减。返回从各工事抽走的人数。
fn pull_from_work(city: &mut City, group: Id, count: u32) -> BTreeMap<Id, u32> {
    let mut posts = BTreeMap::new();
    let mut short = count.saturating_sub(city.unmanaged(group));
    let size_after = city.groups[&group].size - count;
    for a in city.assignments.values_mut().rev() {
        let Some(workers) = a.workers.get_mut(&group) else {
            continue;
        };
        if a.task.uses_labor() {
            if short == 0 {
                continue;
            }
            let pulled = (*workers).min(short);
            *workers -= pulled;
            short -= pulled;
            posts.insert(a.id, pulled);
        } else {
            *workers = (*workers).min(size_after);
        }
        if *workers == 0 {
            a.workers.remove(&group);
        }
    }
    posts
}

/// 遣散：军队回到编成它的城的地上，就能一键遣散。人回到原来的人群，原来的工事还在就回原来的岗位，
/// 装备还回府库。
pub fn disband(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    army_id: Id,
) -> Result<Vec<Id>, CommandError> {
    let army = state
        .armies
        .get(&army_id)
        .ok_or(CommandError::NoSuchArmy(army_id))?;
    if army.commander != actor {
        return Err(CommandError::NotCommander(army_id));
    }
    let city = state
        .cities
        .get(&army.city)
        .ok_or(CommandError::NoSuchCity(army.city))?;
    if city.tile_at(army.pos).is_none() {
        return Err(CommandError::NotHome(army_id));
    }
    let mut army = crate::supply::remove(state, config, army_id).expect("checked above");
    let mut next_id = state.next_id;
    let city = state.cities.get_mut(&army.city).expect("checked above");
    for squad in std::mem::take(&mut army.squads) {
        return_squad(city, config, &squad, &mut next_id);
    }
    state.next_id = next_id;
    Ok(vec![])
}

/// 一队兵回到城里：人回原来的人群，原来的工事还在就回原来的岗位，装备还回府库。
pub(crate) fn return_squad(city: &mut City, config: &Config, squad: &Squad, next_id: &mut Id) {
    let group = return_people(city, squad, next_id);
    for (assignment, count) in &squad.posts {
        let capacity = city
            .assignments
            .get(assignment)
            .map_or(0, |a| city.official_capacity(a.shi, config));
        let Some(a) = city.assignments.get_mut(assignment) else {
            continue;
        };
        if !a.task.can_be_done_by(config, &city.groups[&group]) {
            continue;
        }
        let room = capacity.saturating_sub(a.worker_count());
        let back = (*count).min(room);
        if back > 0 {
            *a.workers.entry(group).or_insert(0) += back;
        }
    }
    city.treasury.add_all(&squad.gear);
}

/// 人回到原来的人群；原来的人群技能槽变了，就并入技能槽和文化图腾都相同的人群，没有就重新成群。
fn return_people(city: &mut City, squad: &Squad, next_id: &mut Id) -> Id {
    let home = city
        .groups
        .get(&squad.group)
        .filter(|g| g.skills == squad.skills && g.totem == squad.totem)
        .or_else(|| {
            city.groups
                .values()
                .find(|g| g.skills == squad.skills && g.totem == squad.totem)
        })
        .map(|g| g.id);
    let n = squad.count;
    if let Some(group) = home.and_then(|id| city.groups.get_mut(&id)) {
        let total = (group.size + n).max(1);
        let mix = |a: Fx, b: Fx| (a * group.size + b * n) / total;
        group.culture = mix(group.culture, squad.culture);
        group.happiness = mix(group.happiness, squad.happiness);
        group.loyalty = mix(group.loyalty, squad.loyalty);
        group.hostility = mix(group.hostility, squad.hostility);
        group.size += squad.count;
        return group.id;
    }
    let id = *next_id;
    *next_id += 1;
    city.groups.insert(
        id,
        PopGroup {
            id,
            name: format!("{}·归营", squad.totem),
            size: squad.count,
            skills: squad.skills.clone(),
            totem: squad.totem.clone(),
            culture: squad.culture,
            happiness: squad.happiness,
            loyalty: squad.loyalty,
            growth_carry: Fx::ZERO,
            hostility: squad.hostility,
        },
    );
    id
}

/// 城换了主人：这座城的军队跟着归新主人统领。
pub fn transfer(state: &mut GameState, city: Id, to: Id) {
    for army in state.armies.values_mut().filter(|a| a.city == city) {
        army.commander = to;
    }
}

/// 每座城要供养的军中人数：(脱产精兵, 征发来的兵)。出征的军队由粮道供粮，不算在内；
/// 军队没出征时，它粮道上的辎重兵由起点城供养。
pub fn mouths(state: &GameState, config: &Config) -> BTreeMap<Id, (u32, u32)> {
    let mut mouths: BTreeMap<Id, (u32, u32)> = BTreeMap::new();
    let mut add = |city: Id, squad: &Squad| {
        let entry = mouths.entry(city).or_default();
        if squad.is_elite(config) {
            entry.0 += squad.count;
        } else {
            entry.1 += squad.count;
        }
    };
    for army in state.armies.values() {
        if army.on_campaign(state) {
            continue;
        }
        for squad in &army.squads {
            add(army.city, squad);
        }
        if let Some(line) = &army.supply {
            add(line.from, &line.convoy);
        }
    }
    mouths
}

/// 走进一格的耗时：地形要能走；别人的城要有通行权，没有就绕开。交战中的敌城，城墙破了就能进去；
/// 硬闯的行军不管通行权，但进不了城墙立着的城。
/// 别人的军队占着的格子走不进去，敌军除外：找路时照样算进去，走到跟前就开打。
fn step_cost(state: &GameState, config: &Config, army: &Army, pos: Hex) -> Option<u32> {
    let cell = state.map.get(pos)?;
    let cost = army.terrain_cost(config, &cell.terrain)?;
    if !state.can_pass(army.commander, pos, false) {
        let city = &state.cities[&state.city_at(pos)?];
        let enemy = city
            .holder()
            .is_some_and(|h| state.at_war(army.commander, h));
        if !(enemy || army.force) || city.wall_standing(config) {
            return None;
        }
    }
    if state.armies.values().any(|a| {
        a.pos == pos && a.commander != army.commander && !state.at_war(a.commander, army.commander)
    }) {
        return None;
    }
    Some(cost)
}

/// 下令行军：只指定目的地，自动按耗时最少的路线走，当回合用掉剩下的行军力，走不到的之后的回合接着走。
/// 目的地就是现在的位置时，取消行军。
pub fn order_march(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    army_id: Id,
    to: Hex,
    force: bool,
) -> Result<Vec<Id>, CommandError> {
    let army = state
        .armies
        .get(&army_id)
        .ok_or(CommandError::NoSuchArmy(army_id))?;
    if army.commander != actor {
        return Err(CommandError::NotCommander(army_id));
    }
    let mut planned = army.clone();
    planned.force = force;
    if to != army.pos
        && state
            .map
            .shortest_path(army.pos, to, |p| step_cost(state, config, &planned, p))
            .is_none()
    {
        return Err(CommandError::NoPath(to));
    }
    let army = state.armies.get_mut(&army_id).expect("checked above");
    army.dest = (to != army.pos).then_some(to);
    army.force = force;
    let mut events = Vec::new();
    march(state, config, army_id, &mut events);
    state.pending_events.extend(events);
    Ok(vec![])
}

/// 沿着耗时最少的路线往目的地走，直到行军力用完。每走一步重新找路，路被挡住就停下等新的命令。
/// 行军力不够走下一格时，满行军力也能走一格，免得慢的兵种被困住。
pub fn march(state: &mut GameState, config: &Config, army_id: Id, events: &mut Vec<Event>) {
    loop {
        let army = &state.armies[&army_id];
        let Some(dest) = army.dest else { return };
        let in_sight = crate::vision::enemies_in_sight(state, config, army);
        let path = state
            .map
            .shortest_path(army.pos, dest, |p| step_cost(state, config, army, p));
        let Some(next) = path.as_ref().and_then(|p| p.first().copied()) else {
            let name = army_name(state, army);
            events.push(
                Event::new(format!("{name}去 {dest} 的路被挡住了，停在 {}", army.pos))
                    .to([army.commander]),
            );
            state.armies.get_mut(&army_id).expect("exists").dest = None;
            return;
        };
        // 下一格就是敌军：不走进去，直接开打
        if state
            .armies
            .values()
            .any(|a| a.pos == next && state.at_war(a.commander, army.commander))
        {
            crate::combat::engage(state, config, army_id, events);
            state.armies.get_mut(&army_id).expect("exists").dest = None;
            return;
        }
        let cost = step_cost(state, config, army, next).expect("on the path");
        // 硬闯没有通行权的城：对城主宣战，宣不了战就停下
        let holder = state.city_at(next).and_then(|c| state.cities[&c].holder());
        if !state.can_pass(army.commander, next, false)
            && let Some(holder) = holder
            && !state.at_war(army.commander, holder)
        {
            let commander = army.commander;
            let target = crate::war::Party::Person { id: holder };
            if let Err(e) = crate::war::declare(state, config, commander, target, events) {
                let name = army_name(state, &state.armies[&army_id]);
                events.push(Event::new(format!("{name}不能硬闯 {next}：{e}")).to([commander]));
                state.armies.get_mut(&army_id).expect("exists").dest = None;
                return;
            }
        }
        let army = &state.armies[&army_id];
        let full = army.moves(config);
        if army.moves_left == 0 || (army.moves_left < cost && army.moves_left < full) {
            return;
        }
        let army = state.armies.get_mut(&army_id).expect("exists");
        army.pos = next;
        army.on_wall = false;
        army.moves_left = army.moves_left.saturating_sub(cost);
        if next == dest {
            army.dest = None;
        }
        // 走到敌军旁边就停下，直接开打
        if crate::combat::engage(state, config, army_id, events) {
            return;
        }
        // 新的敌军进入视野：停下等新的命令
        let army = &state.armies[&army_id];
        if army.dest.is_some()
            && crate::vision::enemies_in_sight(state, config, army)
                .iter()
                .any(|id| !in_sight.contains(id))
        {
            let name = army_name(state, army);
            events.push(
                Event::new(format!("{name}在 {} 发现敌军，停下待命", army.pos))
                    .to([army.commander]),
            );
            state.armies.get_mut(&army_id).expect("exists").dest = None;
            return;
        }
        if state.armies[&army_id].dest.is_none() {
            return;
        }
    }
}

/// 新回合开始：所有军队的行军力回满，有目的地的接着走。
pub fn start_turn(state: &mut GameState, config: &Config, events: &mut Vec<Event>) {
    let ids: Vec<Id> = state.armies.keys().copied().collect();
    for id in &ids {
        let army = state.armies.get_mut(id).expect("listed");
        let full = army.moves(config);
        army.moves_left = full;
        army.attacked = false;
    }
    for city in state.cities.values_mut() {
        city.wall_attacked = false;
        city.plundered = false;
    }
    for id in ids {
        march(state, config, id, events);
    }
}

pub(crate) fn army_name(state: &GameState, army: &Army) -> String {
    let city = state
        .cities
        .get(&army.city)
        .map_or("?", |c| c.name.as_str());
    format!("{city}的军队 {}", army.id)
}
