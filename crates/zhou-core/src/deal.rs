//! 赠与、索取、交易：一方提出「我给你什么、我要你什么」，对方接受就立即成交，不需要押运。
//! 只给不要是赠与，只要不给是索取，两边都有是交易。可以拿来往来的有物资、整座城、通行权。
//! 提议只在发起的当回合有效，回合结束时还没答复就自动作废。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::command::CommandError;
use crate::config::Config;
use crate::inventory::Inventory;
use crate::state::{CityStatus, GameState, Id};

/// 一方拿出来的东西：私库里的物资、名下城市府库里的物资、名下的城市。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stake {
    #[serde(default)]
    pub private: Inventory,
    /// 城市 id → 从这座城的府库出的物资。城必须是自己的，不用管理者同意
    #[serde(default)]
    pub treasuries: BTreeMap<Id, Inventory>,
    /// 整座送出去的城市
    #[serde(default)]
    pub cities: Vec<Id>,
    /// 给对方的通行权：对方的军队可以从这些城经过
    #[serde(default)]
    pub passage: Vec<Passage>,
}

/// 一座城的通行权，`turns` 回合后到期，不写时按配置的默认期限。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Passage {
    pub city: Id,
    #[serde(default)]
    pub turns: Option<u32>,
}

impl Stake {
    pub fn is_empty(&self) -> bool {
        self.private.iter().next().is_none()
            && self.treasuries.values().all(|t| t.iter().next().is_none())
            && self.cities.is_empty()
            && self.passage.is_empty()
    }
}

/// 一份还没答复的提议。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deal {
    pub id: Id,
    pub from: Id,
    pub to: Id,
    /// 提议的人给出的
    pub give: Stake,
    /// 提议的人要的
    pub take: Stake,
    /// 提议的人收到的物资放进哪座城的府库，None 为私库
    pub into: Option<Id>,
}

impl Deal {
    pub fn kind_name(&self) -> &'static str {
        match (self.give.is_empty(), self.take.is_empty()) {
            (false, true) => "赠与",
            (true, false) => "索取",
            _ => "交易",
        }
    }
}

/// `who` 拿得出这些东西：物资都够，城市都是自己的、不是封地、没有被攻占。
pub(crate) fn check_stake(state: &GameState, config: &Config, who: Id, stake: &Stake) -> Result<(), CommandError> {
    let character = state.characters.get(&who).ok_or(CommandError::NoSuchCharacter(who))?;
    check_goods(config, &stake.private)?;
    for (good, amount) in stake.private.iter() {
        if character.private_store.get(good) + 1e-9 < *amount {
            return Err(CommandError::NotEnoughInPrivateStore(good.clone()));
        }
    }
    for (city_id, goods) in &stake.treasuries {
        let city = owned_city(state, who, *city_id)?;
        check_goods(config, goods)?;
        for (good, amount) in goods.iter() {
            if city.treasury.get(good) + 1e-9 < *amount {
                return Err(CommandError::NotEnoughInTreasury(good.clone()));
            }
        }
    }
    for (i, city_id) in stake.cities.iter().enumerate() {
        if stake.cities[..i].contains(city_id) {
            return Err(CommandError::DuplicateCity(*city_id));
        }
        // 封地是封君封给自己的，不能转手
        if owned_city(state, who, *city_id)?.liege.is_some() {
            return Err(CommandError::AlreadyFief);
        }
        // 都城不能送人、不能拿来交易
        if state.countries.values().any(|c| c.capital == Some(*city_id)) {
            return Err(CommandError::CapitalCannotChangeHands(*city_id));
        }
    }
    for (i, p) in stake.passage.iter().enumerate() {
        // 同一座城的通行权只能给一次，也不能连城带通行权一起给
        if stake.passage[..i].iter().any(|q| q.city == p.city) || stake.cities.contains(&p.city) {
            return Err(CommandError::DuplicateCity(p.city));
        }
        owned_city(state, who, p.city)?;
        if p.turns == Some(0) {
            return Err(CommandError::ZeroPassageTurns);
        }
    }
    Ok(())
}

/// 收到的物资放进 `into` 这座城的府库：城必须是自己的，而且不在这次送出去的城里。
pub(crate) fn check_into(state: &GameState, who: Id, into: Option<Id>, giving: &Stake) -> Result<(), CommandError> {
    let Some(city) = into else { return Ok(()) };
    owned_city(state, who, city)?;
    if giving.cities.contains(&city) {
        return Err(CommandError::DuplicateCity(city));
    }
    Ok(())
}

/// 两边都再检查一遍，然后成交。
pub(crate) fn settle(state: &mut GameState, config: &Config, deal: &Deal, accept_into: Option<Id>) -> Result<(), CommandError> {
    check_stake(state, config, deal.from, &deal.give)?;
    check_stake(state, config, deal.to, &deal.take)?;
    check_into(state, deal.from, deal.into, &deal.give)?;
    check_into(state, deal.to, accept_into, &deal.take)?;
    transfer(state, config, deal.from, deal.to, &deal.give, accept_into);
    transfer(state, config, deal.to, deal.from, &deal.take, deal.into);
    Ok(())
}

fn transfer(state: &mut GameState, config: &Config, from: Id, to: Id, stake: &Stake, into: Option<Id>) {
    let mut goods = Inventory::default();
    let giver = state.characters.get_mut(&from).expect("checked");
    for (good, amount) in stake.private.iter() {
        goods.add(good, giver.private_store.take(good, *amount));
    }
    for (city, items) in &stake.treasuries {
        let city = state.cities.get_mut(city).expect("checked");
        for (good, amount) in items.iter() {
            goods.add(good, city.treasury.take(good, *amount));
        }
    }
    match into {
        Some(city) => state.cities.get_mut(&city).expect("checked").treasury.add_all(&goods),
        None => state.characters.get_mut(&to).expect("checked").private_store.add_all(&goods),
    }
    // 已经有通行权的，到期时间取晚的那个
    for p in &stake.passage {
        let until = state.turn + p.turns.unwrap_or(config.passage.default_turns);
        let entry = state.cities.get_mut(&p.city).expect("checked").passage.entry(to).or_insert(0);
        *entry = (*entry).max(until);
    }
    for city in &stake.cities {
        give_city(state, config, *city, to);
    }
}

/// 城市整座易手：所有权直接转给对方，不形成封君封臣关系。
/// 管理权归新主人；他已经在管别的城时，这座城先没有管理者，等他任命别人。
fn give_city(state: &mut GameState, config: &Config, city_id: Id, to: Id) {
    let manages_other = state.cities.values().any(|c| c.id != city_id && c.manager == Some(to));
    let city = state.cities.get_mut(&city_id).expect("checked");
    city.owner = Some(to);
    city.fief_offer = None;
    city.manager_offer = None;
    city.quota = None;
    // 通行权是原所有者给的，换了主人就作废
    city.passage.clear();
    if city.manager == Some(to) {
        return;
    }
    if manages_other {
        city.manager = None;
        for group in city.groups.values_mut() {
            group.loyalty = config.loyalty.reset;
        }
    } else {
        city.change_manager(to, config);
    }
}

fn owned_city(state: &GameState, who: Id, city: Id) -> Result<&crate::state::City, CommandError> {
    let c = state.cities.get(&city).ok_or(CommandError::NoSuchCity(city))?;
    if c.owner != Some(who) {
        return Err(CommandError::NotOwner(city));
    }
    if c.status == CityStatus::Occupied {
        return Err(CommandError::CityOccupied(city));
    }
    Ok(c)
}

fn check_goods(config: &Config, goods: &Inventory) -> Result<(), CommandError> {
    for (good, amount) in goods.iter() {
        if !config.goods.contains_key(good) {
            return Err(CommandError::NoSuchGood(good.clone()));
        }
        if *amount <= 0.0 {
            return Err(CommandError::ZeroCount);
        }
    }
    Ok(())
}
