//! 作战与攻城，规则照文明系列：相邻的格子近战，隔一格远程；一支军队每回合只能攻击一次，
//! 攻击后当回合不能再走（骑兵除外）；近战时守方自动还手，远程攻击对方不能还手。
//!
//! 战斗力 = 人数 × 兵种的攻击或防御 × 装备倍率 × 加成。每点战斗力每次交战杀死对方 `lethality` 人。
//! 没有随机，结果可复现。
//!
//! 攻城：城墙立着时只能打城墙，墙上的守军还手，城墙一破，墙上的守军随之覆灭；
//! 墙里其余的守军要另外打败，打光了才能打城中心，城中心打下来城就被攻占。

use serde::{Deserialize, Serialize};

use crate::army::{Army, Squad, army_name};
use crate::command::CommandError;
use crate::config::Config;
use crate::event::Event;
use crate::map::Hex;
use crate::state::{Character, City, CityStatus, GameState, Id};

impl Squad {
    /// 近战攻击力。
    pub fn attack(&self, config: &Config) -> f64 {
        self.count as f64 * config.units[&self.unit].attack * self.gear_mult(config)
    }

    /// 防御力，带甲的部分按比例加成。
    pub fn defense(&self, config: &Config) -> f64 {
        let def = &config.units[&self.unit];
        let armored = self.carried_share(config, "armor");
        self.count as f64
            * def.defense
            * self.gear_mult(config)
            * (1.0 + config.combat.armor_bonus * armored)
    }

    /// 远程攻击力：只算带了弓矢的人。
    pub fn ranged(&self, config: &Config) -> f64 {
        let def = &config.units[&self.unit];
        if def.ranged <= 0.0 {
            return 0.0;
        }
        self.count as f64 * self.carried_share(config, "bow") * def.ranged * self.gear_mult(config)
    }

    /// 可选装备够几成的人用，精品也算。
    fn carried_share(&self, config: &Config, good: &str) -> f64 {
        let per = config.units[&self.unit].optional.get(good);
        if per <= 0.0 || self.count == 0 {
            return 0.0;
        }
        let fine = config.goods.get(good).and_then(|g| g.fine.as_deref());
        let have = self.gear.get(good) + fine.map_or(0.0, |f| self.gear.get(f));
        (have / (per * self.count as f64)).min(1.0)
    }
}

impl Army {
    /// 近战攻击力，断粮时打折。
    pub fn attack(&self, config: &Config) -> f64 {
        self.squads.iter().map(|s| s.attack(config)).sum::<f64>() * self.fed_mult(config)
    }

    pub fn ranged(&self, config: &Config) -> f64 {
        self.squads.iter().map(|s| s.ranged(config)).sum::<f64>() * self.fed_mult(config)
    }

    /// 防御力，加上所在地形的加成，断粮时打折。
    pub fn defense(&self, config: &Config, terrain: Option<&str>) -> f64 {
        let bonus = terrain
            .and_then(|t| config.terrains.get(t))
            .map_or(0.0, |t| t.defense_bonus);
        self.squads.iter().map(|s| s.defense(config)).sum::<f64>()
            * (1.0 + bonus)
            * self.fed_mult(config)
    }

    /// 带着的攻城机架数。
    pub fn engines(&self, config: &Config) -> f64 {
        self.squads
            .iter()
            .map(|s| s.gear.get(&config.siege.engine_good))
            .sum()
    }

    /// 死了 `dead` 人：按人数比例摊到各队，装备跟着人一起丢。返回实际死了几人。
    fn lose(&mut self, dead: f64) -> u32 {
        let size = self.size();
        if size == 0 {
            return 0;
        }
        let dead = (dead.round() as u32).min(size);
        let mut left = dead;
        let n = self.squads.len();
        for (i, squad) in self.squads.iter_mut().enumerate() {
            let share = if i + 1 == n {
                left
            } else {
                ((dead as f64 * squad.count as f64 / size as f64).round() as u32).min(left)
            };
            let share = share.min(squad.count);
            let keep = (squad.count - share) as f64 / squad.count.max(1) as f64;
            for amount in squad.gear.0.values_mut() {
                *amount *= keep;
            }
            squad.gear.0.retain(|_, a| *a > 1e-9);
            squad.count -= share;
            left -= share;
        }
        self.squads.retain(|s| s.count > 0);
        dead - left
    }
}

/// 劫掠的对象。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlunderTarget {
    /// 拿走府库的全部物资
    Treasury,
    /// 拿走民间财富：人群愉悦度、对占领者的忠诚度下降，占领者扣风评
    Citizens,
}

/// 守城的一方是不是 `who` 的敌人：没有主人的城谁都能打。
fn hostile_city(state: &GameState, city: &City, who: Id) -> bool {
    city.holder().is_none_or(|h| state.at_war(who, h))
}

/// 下令攻击某一格：那一格是敌城的地就攻城，有敌军就打敌军。`ranged` 为远程攻击（弓矢、攻城机）。
pub fn attack(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    army_id: Id,
    at: Hex,
    ranged: bool,
) -> Result<Vec<Id>, CommandError> {
    let army = state
        .armies
        .get(&army_id)
        .ok_or(CommandError::NoSuchArmy(army_id))?;
    if army.commander != actor {
        return Err(CommandError::NotCommander(army_id));
    }
    if army.attacked || army.moves_left == 0 {
        return Err(CommandError::AlreadyActed(army_id));
    }
    let distance = army.pos.distance(at);
    let in_range = if ranged {
        (1..=config.combat.ranged_range).contains(&distance)
    } else {
        distance == 1
    };
    if !in_range {
        return Err(CommandError::OutOfRange(at));
    }
    let mut events = Vec::new();
    strike(state, config, army_id, at, ranged, &mut events)?;
    state.pending_events.extend(events);
    Ok(vec![])
}

/// 结算一次攻击。
fn strike(
    state: &mut GameState,
    config: &Config,
    army_id: Id,
    at: Hex,
    ranged: bool,
    events: &mut Vec<Event>,
) -> Result<(), CommandError> {
    let army = &state.armies[&army_id];
    let commander = army.commander;
    let power = if ranged {
        army.ranged(config)
    } else {
        army.attack(config)
    };
    let city = state
        .city_at(at)
        .map(|id| &state.cities[&id])
        .filter(|c| hostile_city(state, c, commander));

    // 城墙立着：只能打城墙，墙里的守军打不到
    if let Some(city) = city
        && city.wall_standing(config)
    {
        let engines = if ranged { army.engines(config) } else { 0.0 };
        if power <= 0.0 && engines <= 0.0 {
            return Err(CommandError::CannotAttack(army_id));
        }
        let (city_id, holder) = (city.id, city.holder());
        if let Some(holder) = holder {
            crate::war::note_attack(state, config, commander, holder, Some(city_id), events);
        }
        siege_wall(
            state, config, army_id, city_id, power, engines, ranged, events,
        );
        finish_attack(state, config, army_id);
        return Ok(());
    }
    if power <= 0.0 {
        return Err(CommandError::CannotAttack(army_id));
    }
    let enemy = state
        .armies
        .values()
        .find(|a| a.pos == at && state.at_war(commander, a.commander))
        .map(|a| a.id);
    if let Some(enemy) = enemy {
        let target = state.armies[&enemy].commander;
        let city_id = city.map(|c| c.id);
        crate::war::note_attack(state, config, commander, target, city_id, events);
        battle(state, config, army_id, enemy, power, ranged, events);
        finish_attack(state, config, army_id);
        return Ok(());
    }
    let Some(city) = city else {
        // 没有敌军也没有敌城：敌人的粮道从这里过，就袭击粮道
        if state.city_at(at).is_none() && crate::supply::raid(state, config, commander, at, events)
        {
            finish_attack(state, config, army_id);
            return Ok(());
        }
        return Err(CommandError::NothingToAttack(at));
    };
    // 墙里还有守军，要先打败他们
    let holder = city.holder();
    if state
        .armies
        .values()
        .any(|a| Some(a.commander) == holder && city.tile_at(a.pos).is_some())
    {
        return Err(CommandError::GarrisonRemains(city.id));
    }
    let city_id = city.id;
    if let Some(holder) = holder {
        crate::war::note_attack(state, config, commander, holder, Some(city_id), events);
    }
    let city = state.cities.get_mut(&city_id).expect("found above");
    let engines = if ranged {
        state.armies[&army_id].engines(config)
    } else {
        0.0
    };
    city.center_damage += power * config.siege.center_rate + engines * config.siege.engine_damage;
    let name = army_name(state, &state.armies[&army_id]);
    events.push(
        Event::new(format!("{name}攻打{}的城中心", state.cities[&city_id].name))
            .to([commander])
            .to(holder)
            .at(at),
    );
    if state.cities[&city_id].center_damage >= config.siege.center_hp {
        capture(state, config, city_id, commander, events);
        if !ranged {
            let army = state.armies.get_mut(&army_id).expect("exists");
            army.pos = at;
            army.on_wall = false;
        }
    }
    finish_attack(state, config, army_id);
    Ok(())
}

/// 攻击之后：当回合不能再攻击，也不能再走（骑兵除外）。
fn finish_attack(state: &mut GameState, config: &Config, army_id: Id) {
    let Some(army) = state.armies.get_mut(&army_id) else {
        return;
    };
    army.attacked = true;
    army.dest = None;
    let keeps_moving = army
        .squads
        .iter()
        .all(|s| config.units[&s.unit].move_after_attack);
    if !keeps_moving {
        army.moves_left = 0;
    }
}

/// 两军交战：近战时守方用防御力还手，远程时对方不能还手。
fn battle(
    state: &mut GameState,
    config: &Config,
    attacker: Id,
    defender: Id,
    power: f64,
    ranged: bool,
    events: &mut Vec<Event>,
) {
    let k = config.combat.lethality;
    let def = &state.armies[&defender];
    let terrain = state.map.get(def.pos).map(|c| c.terrain.as_str());
    let back = if ranged {
        0.0
    } else {
        def.defense(config, terrain) * k
    };
    let (a_name, d_name) = (
        army_name(state, &state.armies[&attacker]),
        army_name(state, def),
    );
    let killed = state
        .armies
        .get_mut(&defender)
        .expect("exists")
        .lose(power * k);
    let lost = state.armies.get_mut(&attacker).expect("exists").lose(back);
    let how = if ranged { "远程攻击" } else { "进攻" };
    events.push(
        Event::new(format!(
            "{a_name}{how}{d_name}：杀敌 {killed} 人，折损 {lost} 人"
        ))
        .to([
            state.armies[&attacker].commander,
            state.armies[&defender].commander,
        ])
        .at(state.armies[&defender].pos),
    );
    bury(state, config, &[attacker, defender], events);
}

/// 打城墙：攻城机和战斗力都能打掉防御值；墙上的守军还手，打远处的只能用弓矢。城墙破了，墙上的守军覆灭。
#[allow(clippy::too_many_arguments)]
fn siege_wall(
    state: &mut GameState,
    config: &Config,
    army_id: Id,
    city_id: Id,
    power: f64,
    engines: f64,
    ranged: bool,
    events: &mut Vec<Event>,
) {
    let s = &config.siege;
    let city = &state.cities[&city_id];
    let holder = city.holder();
    let on_wall: Vec<Id> = state
        .armies
        .values()
        .filter(|a| a.on_wall && Some(a.commander) == holder && city.tile_at(a.pos).is_some())
        .map(|a| a.id)
        .collect();
    let attacker_pos = state.armies[&army_id].pos;
    let near = city
        .tiles
        .values()
        .any(|t| t.pos.distance(attacker_pos) <= config.combat.ranged_range);
    let back: f64 = on_wall
        .iter()
        .map(|id| {
            let a = &state.armies[id];
            if ranged {
                a.ranged(config)
            } else {
                a.defense(config, None)
            }
        })
        .sum::<f64>()
        * (1.0 + config.combat.wall_bonus)
        * config.combat.lethality;
    let back = if near { back } else { 0.0 };

    let damage = power * s.wall_rate + engines * s.engine_damage;
    let name = army_name(state, &state.armies[&army_id]);
    let city = state.cities.get_mut(&city_id).expect("exists");
    city.wall_damage = (city.wall_damage + damage).min(s.wall_hp);
    city.wall_attacked = true;
    let broken = city.wall_damage >= s.wall_hp;
    let city_name = city.name.clone();
    let lost = state.armies.get_mut(&army_id).expect("exists").lose(back);
    events.push(
        Event::new(format!(
            "{name}攻打{city_name}的城墙，打掉 {damage:.1}，墙上守军还手，折损 {lost} 人"
        ))
        .to([state.armies[&army_id].commander])
        .to(holder)
        .at(state.cities[&city_id].seat),
    );
    if broken {
        for id in &on_wall {
            crate::supply::remove(state, config, *id);
        }
        events.push(
            Event::new(format!("{city_name}的城墙被攻破，墙上的守军随之覆灭"))
                .to([state.armies[&army_id].commander])
                .to(holder)
                .at(state.cities[&city_id].seat),
        );
    }
    bury(state, config, &[army_id], events);
}

/// 打光了的军队从地图上去掉。
fn bury(state: &mut GameState, config: &Config, ids: &[Id], events: &mut Vec<Event>) {
    for id in ids {
        if state.armies.get(id).is_some_and(|a| a.size() == 0) {
            let army = &state.armies[id];
            let event = Event::new(format!("{}全军覆没", army_name(state, army)))
                .to([army.commander])
                .at(army.pos);
            crate::supply::remove(state, config, *id);
            events.push(event);
        }
    }
}

/// 城中心被打下来：城被攻占。占领者用它的府库、收税、征兵，但没有所有权；
/// 原来的管理者失去管理权，占领者要在当回合选一个人物或者士人来接管。原所有者打回来就收复。
fn capture(state: &mut GameState, config: &Config, city_id: Id, by: Id, events: &mut Vec<Event>) {
    let by_name = state
        .characters
        .get(&by)
        .map_or("?", |c| c.name.as_str())
        .to_string();
    let manages_other = state
        .cities
        .values()
        .any(|c| c.id != city_id && c.manager == Some(by));
    let city = state.cities.get_mut(&city_id).expect("exists");
    city.center_damage = 0.0;
    city.manager_offer = None;
    city.fief_offer = None;
    if city.owner == Some(by) {
        city.status = CityStatus::Ruled;
        city.occupier = None;
        if manages_other {
            city.manager = None;
        } else {
            city.change_manager(by, config);
        }
        events.push(
            Event::new(format!("{by_name}收复了{}", city.name))
                .to([by])
                .to(city.owner)
                .at(city.seat),
        );
        return;
    }
    city.status = CityStatus::Occupied;
    city.occupier = Some(by);
    city.manager = None;
    city.quota = None;
    events.push(
        Event::new(format!("{by_name}攻占了{}，要在本回合选人接管", city.name))
            .to([by])
            .to(city.owner)
            .at(city.seat),
    );
}

/// 接管被攻占的城的人：一个人物，或者占领者自己管的城里的一名士人（他就此出仕，成为人物）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Appointee {
    Character { id: Id },
    Shi { city: Id, shi: Id },
}

/// 占领者选人接管被攻占的城。人物要是占领者自己或者同一国的人，并且没有在管别的城；
/// 士人要从占领者自己管理的城里出，不能正在主事工事。城中人群对新管理者的初始忠诚度：
/// 管理者风评越高越忠诚；对旧主越不忠诚，对新管理者越忠诚。
pub fn appoint_occupation_manager(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    city_id: Id,
    appointee: &Appointee,
) -> Result<Vec<Id>, CommandError> {
    let city = state
        .cities
        .get(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
    if city.status != CityStatus::Occupied || city.occupier != Some(actor) {
        return Err(CommandError::NotOccupier(city_id));
    }
    let nationality = |id: Id| state.characters.get(&id).and_then(|c| c.nationality);
    let manager = match appointee {
        Appointee::Character { id } => {
            if !state.characters.contains_key(id) {
                return Err(CommandError::NoSuchCharacter(*id));
            }
            if *id != actor
                && (nationality(*id).is_none() || nationality(*id) != nationality(actor))
            {
                return Err(CommandError::DifferentCountry(*id));
            }
            if state
                .cities
                .values()
                .any(|c| c.id != city_id && c.manager == Some(*id))
            {
                return Err(CommandError::ManagesAnotherCity(*id));
            }
            *id
        }
        Appointee::Shi { city: from, shi } => {
            let home = state
                .cities
                .get(from)
                .ok_or(CommandError::NoSuchCity(*from))?;
            if home.manager != Some(actor) {
                return Err(CommandError::NotManager(*from));
            }
            let official = home
                .officials
                .get(shi)
                .ok_or(CommandError::NoSuchOfficial(*shi))?;
            if home.assignment_of(*shi).is_some() {
                return Err(CommandError::OfficialBusy(*shi));
            }
            // 出仕：离开原来的城，成为有名字的人物，随占领者的国籍
            let mut character = Character::new(0, official.name.clone(), "none", 0.0);
            character.nationality = nationality(actor);
            let id = state.alloc_id();
            character.id = id;
            state.characters.insert(id, character);
            state
                .cities
                .get_mut(from)
                .expect("checked")
                .officials
                .remove(shi);
            id
        }
    };
    let reputation = state.characters[&manager].reputation;
    let o = &config.occupation;
    let city = state.cities.get_mut(&city_id).expect("checked");
    city.manager = Some(manager);
    for group in city.groups.values_mut() {
        let grudge = ((o.grudge_below - group.loyalty).max(0.0) * o.grudge_mult).min(o.grudge_max);
        let trust = (reputation * o.reputation_mult).min(o.reputation_max);
        group.loyalty = (o.base_loyalty + trust + grudge).clamp(0.0, config.loyalty.max);
    }
    state.settle_locations();
    Ok(vec![manager])
}

/// 行军途中走到敌军旁边：停下，直接开打。返回是否遇敌。
pub fn engage(
    state: &mut GameState,
    config: &Config,
    army_id: Id,
    events: &mut Vec<Event>,
) -> bool {
    let army = &state.armies[&army_id];
    let enemy = state
        .armies
        .values()
        .filter(|a| a.pos.is_adjacent(army.pos) && state.at_war(army.commander, a.commander))
        .filter(|a| {
            // 立着的城墙里的敌军打不到
            state
                .city_at(a.pos)
                .is_none_or(|c| !state.cities[&c].wall_standing(config))
        })
        .map(|a| (a.id, a.pos))
        .next();
    let Some((_, pos)) = enemy else {
        return false;
    };
    let can_fight = !army.attacked && army.moves_left > 0 && army.attack(config) > 0.0;
    state.armies.get_mut(&army_id).expect("exists").dest = None;
    if can_fight {
        let _ = strike(state, config, army_id, pos, false, events);
    }
    true
}

/// 进驻或离开城墙：只能在自己守的、城墙立着的城里。
pub fn man_wall(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    army_id: Id,
    on: bool,
) -> Result<Vec<Id>, CommandError> {
    let army = state
        .armies
        .get(&army_id)
        .ok_or(CommandError::NoSuchArmy(army_id))?;
    if army.commander != actor {
        return Err(CommandError::NotCommander(army_id));
    }
    if on {
        let city = state.city_at(army.pos).map(|id| &state.cities[&id]);
        if !city.is_some_and(|c| c.holder() == Some(actor) && c.wall_standing(config)) {
            return Err(CommandError::NoWall);
        }
    }
    state.armies.get_mut(&army_id).expect("checked").on_wall = on;
    Ok(vec![])
}

/// 占领者劫掠：从府库或民间拿走这几类物品（不挑就全拿），进自己的私库。每座城每回合只能劫掠一次。
/// 劫掠市民的代价按抢走的价值占民间财富的比例打折。
pub fn plunder(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    city_id: Id,
    target: PlunderTarget,
    categories: &[String],
) -> Result<Vec<Id>, CommandError> {
    if let Some(c) = categories
        .iter()
        .find(|c| !config.categories.contains_key(*c))
    {
        return Err(CommandError::NoSuchCategory(c.clone()));
    }
    if !state.characters.contains_key(&actor) {
        return Err(CommandError::NoSuchCharacter(actor));
    }
    let city = state
        .cities
        .get_mut(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
    if city.status != CityStatus::Occupied || city.occupier != Some(actor) {
        return Err(CommandError::NotOccupier(city_id));
    }
    if city.plundered {
        return Err(CommandError::AlreadyPlundered(city_id));
    }
    city.plundered = true;
    let source = match target {
        PlunderTarget::Treasury => &mut city.treasury,
        PlunderTarget::Citizens => &mut city.private_wealth,
    };
    let total = config.value_of(source);
    let picked = |good: &str| {
        categories.is_empty()
            || config
                .goods
                .get(good)
                .is_some_and(|g| categories.contains(&g.category))
    };
    let mut loot = crate::inventory::Inventory::default();
    source.0.retain(|good, amount| {
        if picked(good) {
            loot.add(good, *amount);
            false
        } else {
            true
        }
    });
    let share = if total > 0.0 {
        config.value_of(&loot) / total
    } else {
        0.0
    };
    let p = &config.plunder;
    if target == PlunderTarget::Citizens {
        for group in city.groups.values_mut() {
            group.happiness = (group.happiness - p.happiness_loss * share).max(0.0);
            group.loyalty = (group.loyalty - p.loyalty_loss * share).max(0.0);
        }
    }
    let name = city.name.clone();
    let (insiders, seat) = (city.insiders(), city.seat);
    let who = state.characters.get_mut(&actor).expect("checked");
    who.private_store.add_all(&loot);
    if target == PlunderTarget::Citizens {
        who.reputation -= p.reputation_loss * share;
    }
    let what = match target {
        PlunderTarget::Treasury => "府库",
        PlunderTarget::Citizens => "市民",
    };
    let kinds = if categories.is_empty() {
        "全部财物".to_string()
    } else {
        categories
            .iter()
            .map(|c| config.categories[c].as_str())
            .collect::<Vec<_>>()
            .join("、")
    };
    let event = Event::new(format!("{}劫掠了{name}{what}的{kinds}", who.name))
        .to(insiders)
        .at(seat);
    state.pending_events.push(event);
    Ok(vec![])
}

/// 占领者毁掉一座建筑。
pub fn raze(
    state: &mut GameState,
    actor: Id,
    city_id: Id,
    tile: Id,
    building: &str,
) -> Result<Vec<Id>, CommandError> {
    let city = state
        .cities
        .get_mut(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
    if city.status != CityStatus::Occupied || city.occupier != Some(actor) {
        return Err(CommandError::NotOccupier(city_id));
    }
    let t = city
        .tiles
        .get_mut(&tile)
        .ok_or(CommandError::NoSuchTile(tile))?;
    let i = t
        .buildings
        .iter()
        .position(|b| b.kind == building)
        .ok_or_else(|| CommandError::NoSuchBuilding(building.to_string()))?;
    t.buildings.remove(i);
    Ok(vec![])
}
