//! 粮道：出征的军队靠粮道供粮，粮从起点城的府库出。
//!
//! 军队站在自己的城的地上时由编成它的城供养；离开就是出征，要开一条粮道。粮道从起点城的治所
//! 自动走最短路线到军队所在的格子，经过别人的城要有通行权，绕开敌军，路线每回合重新找。
//! 粮道上只走辎重兵，人数固定，和粮道长短、前线人数无关。粮道越长，军粮消耗越大。
//!
//! 辎重兵被敌军攻击就全部被杀，粮道被袭；找不到路、起点城不在手里、府库的粮不够，都算断粮。
//! 断粮后战斗力每回合下降，断得越久降得越快，降到 0 时军队就地溃散；恢复供粮，战斗力直接回满。

use serde::{Deserialize, Serialize};

use crate::army::{Army, Squad, SquadOrder, army_name, can_command};
use crate::command::CommandError;
use crate::config::Config;
use crate::event::Event;
use crate::map::Hex;
use crate::state::{GameState, Id};

/// 一条粮道。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyLine {
    /// 起点城，粮从这座城的府库出，辎重兵也出自这座城
    pub from: Id,
    /// 走在粮道上的辎重兵
    pub convoy: Squad,
    /// 上次找到的路线：起点城治所之后的每一格，到军队所在的格子为止。找不到路时为空
    #[serde(default)]
    pub path: Vec<Hex>,
}

impl Army {
    /// 出征：不在统帅自己掌握的城的地上。
    pub fn on_campaign(&self, state: &GameState) -> bool {
        state
            .city_at(self.pos)
            .is_none_or(|c| state.cities[&c].holder() != Some(self.commander))
    }

    /// 断粮后战斗力剩下的比例：第 n 个回合断粮时再降 n × starve_rate。
    pub fn fed_mult(&self, config: &Config) -> f64 {
        let h = self.hunger as f64;
        (1.0 - config.supply.starve_rate * h * (h + 1.0) / 2.0).max(0.0)
    }
}

/// 粮道的路线：从起点城的治所到军队所在的格子。没有通行权的城绕开，有别人军队的格子绕开。
pub fn route(state: &GameState, config: &Config, army: &Army, from: Id) -> Option<Vec<Hex>> {
    let seat = state.cities.get(&from)?.seat;
    state.map.shortest_path(seat, army.pos, |p| {
        let cost = state.map.move_cost(p, config)?;
        if p == army.pos {
            return Some(cost);
        }
        if !state.can_pass(army.commander, p, false) {
            return None;
        }
        if state
            .armies
            .values()
            .any(|a| a.pos == p && a.commander != army.commander)
        {
            return None;
        }
        Some(cost)
    })
}

/// 粮道上能被偷袭的格子：不算城里的地（有城守着）和军队所在的格子。
pub fn exposed(state: &GameState, config: &Config, army: &Army) -> Vec<Hex> {
    let Some(line) = &army.supply else {
        return Vec::new();
    };
    route(state, config, army, line.from)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| *p != army.pos && state.city_at(*p).is_none())
        .collect()
}

/// 开粮道：从起点城的一群会「军武」的人里抽出辎重兵。起点城要是自己能统领的城，而且要找得到路。
pub fn open(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    army_id: Id,
    from: Id,
    group: Id,
) -> Result<Vec<Id>, CommandError> {
    let army = state
        .armies
        .get(&army_id)
        .ok_or(CommandError::NoSuchArmy(army_id))?;
    if army.commander != actor {
        return Err(CommandError::NotCommander(army_id));
    }
    if army.supply.is_some() {
        return Err(CommandError::SupplyLineExists(army_id));
    }
    let city = state
        .cities
        .get(&from)
        .ok_or(CommandError::NoSuchCity(from))?;
    if !can_command(city, actor) {
        return Err(CommandError::NotOwner(from));
    }
    if route(state, config, army, from).is_none() {
        return Err(CommandError::NoSupplyRoute(army_id));
    }
    let order = SquadOrder {
        group,
        unit: config.supply.unit.clone(),
        count: config.supply.convoy,
        extras: Vec::new(),
    };
    // 借征发的规矩抽人，抽出来的辎重兵不成军，放到粮道上
    let convoy_army = crate::army::raise(state, config, actor, from, &[order])?[0];
    let convoy = state
        .armies
        .remove(&convoy_army)
        .expect("just raised")
        .squads
        .remove(0);
    let army = state.armies.get_mut(&army_id).expect("checked above");
    army.supply = Some(SupplyLine {
        from,
        convoy,
        path: Vec::new(),
    });
    refresh(state, config, army_id);
    Ok(vec![])
}

/// 撤粮道：辎重兵回起点城，回原来的岗位。
pub fn close(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    army_id: Id,
) -> Result<Vec<Id>, CommandError> {
    let army = state
        .armies
        .get_mut(&army_id)
        .ok_or(CommandError::NoSuchArmy(army_id))?;
    if army.commander != actor {
        return Err(CommandError::NotCommander(army_id));
    }
    let line = army
        .supply
        .take()
        .ok_or(CommandError::NoSupplyLine(army_id))?;
    release(state, config, line);
    Ok(vec![])
}

/// 粮道撤掉或者军队没了：辎重兵回起点城。
pub(crate) fn release(state: &mut GameState, config: &Config, line: SupplyLine) {
    let mut next_id = state.next_id;
    if let Some(city) = state.cities.get_mut(&line.from) {
        crate::army::return_squad(city, config, &line.convoy, &mut next_id);
    }
    state.next_id = next_id;
}

/// 重新找一遍粮道的路线，只用来报告。
pub(crate) fn refresh(state: &mut GameState, config: &Config, army_id: Id) {
    let Some(army) = state.armies.get(&army_id) else {
        return;
    };
    let Some(from) = army.supply.as_ref().map(|l| l.from) else {
        return;
    };
    let path = route(state, config, army, from).unwrap_or_default();
    if let Some(line) = state
        .armies
        .get_mut(&army_id)
        .and_then(|a| a.supply.as_mut())
    {
        line.path = path;
    }
}

pub(crate) fn refresh_all(state: &mut GameState, config: &Config) {
    let ids: Vec<Id> = state.armies.keys().copied().collect();
    for id in ids {
        refresh(state, config, id);
    }
}

/// 敌军攻击 `at` 这一格，粮道从这里过的，辎重兵全被杀。返回有没有袭到粮道。
pub(crate) fn raid(
    state: &mut GameState,
    config: &Config,
    by: Id,
    at: Hex,
    events: &mut Vec<Event>,
) -> bool {
    let hit: Vec<Id> = state
        .armies
        .values()
        .filter(|a| state.at_war(by, a.commander) && exposed(state, config, a).contains(&at))
        .map(|a| a.id)
        .collect();
    for id in &hit {
        let army = state.armies.get_mut(id).expect("listed");
        let line = army.supply.take().expect("has a line");
        let target = army.commander;
        let name = army_name(state, &state.armies[id]);
        events.push(
            Event::new(format!(
                "{name}的粮道在 {at} 被袭，辎重 {} 人被杀，前线断粮",
                line.convoy.count
            ))
            .to([target, by])
            .at(at),
        );
        crate::war::note_attack(state, config, by, target, None, events);
    }
    !hit.is_empty()
}

/// 回合结算：出征的军队从粮道领粮，断粮的战斗力下降，降到 0 就溃散。在城市结算之后，
/// 起点城先养活自己，剩下的再往前线送。
pub(crate) fn feed(state: &mut GameState, config: &Config, events: &mut Vec<Event>) {
    let ids: Vec<Id> = state.armies.keys().copied().collect();
    for id in ids {
        let army = &state.armies[&id];
        if !army.on_campaign(state) {
            state.armies.get_mut(&id).expect("listed").hunger = 0;
            continue;
        }
        let name = army_name(state, army);
        let (commander, pos) = (army.commander, army.pos);
        let fed = match supply_need(state, config, army) {
            Err(why) => {
                events.push(Event::new(format!("{name}断粮：{why}")).to([commander]));
                false
            }
            Ok((from, need)) => {
                let city = state.cities.get_mut(&from).expect("checked");
                if need.iter().all(|(goods, amount)| {
                    goods.iter().map(|g| city.treasury.get(g)).sum::<f64>() + 1e-9 >= *amount
                }) {
                    for (goods, amount) in &need {
                        let mut got = 0.0;
                        for good in goods {
                            got += city.treasury.take(good, amount - got);
                        }
                    }
                    true
                } else {
                    events.push(
                        Event::new(format!("{name}断粮：{}的府库粮食不够", city.name))
                            .to([commander]),
                    );
                    false
                }
            }
        };
        let army = state.armies.get_mut(&id).expect("listed");
        if fed {
            army.hunger = 0;
            continue;
        }
        army.hunger += 1;
        if army.fed_mult(config) <= 0.0 {
            events.push(
                Event::new(format!("{name}断粮太久，就地溃散"))
                    .to([commander])
                    .at(pos),
            );
            remove(state, config, id);
        }
    }
}

/// 一项消耗：用哪几种物品（任意一种都行）、要多少。
type Ration = (Vec<String>, f64);

/// 这支军队这回合要从粮道领的粮：每项消耗用哪几种物品、要多少。
fn supply_need(
    state: &GameState,
    config: &Config,
    army: &Army,
) -> Result<(Id, Vec<Ration>), String> {
    let line = army.supply.as_ref().ok_or("没有粮道")?;
    let city = state.cities.get(&line.from).ok_or("粮道的起点城不在了")?;
    if !can_command(city, army.commander) {
        return Err(format!("{}已经不在手里", city.name));
    }
    let path = route(state, config, army, line.from).ok_or("粮道不通")?;
    let pop = &config.population;
    let mouths = (army.size() + line.convoy.count) as f64;
    let length = 1.0 + path.len() as f64 * config.supply.per_cell;
    Ok((
        line.from,
        pop.consumption
            .iter()
            .map(|r| (r.goods.clone(), mouths * r.amount / pop.per_people * length))
            .collect(),
    ))
}

/// 军队从地图上去掉：打光了、溃散了、城墙破了随之覆灭。粮道上的辎重兵回起点城。
pub(crate) fn remove(state: &mut GameState, config: &Config, id: Id) -> Option<Army> {
    let mut army = state.armies.remove(&id)?;
    if let Some(line) = army.supply.take() {
        release(state, config, line);
    }
    Some(army)
}
