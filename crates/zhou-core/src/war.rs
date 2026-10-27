//! 宣战、国战、讲和与停战。
//!
//! 交战的一方是一个国家或者一个人：国君对别国开战是国与国的战争（国战），全国的人都参与；
//! 封臣之间、无国籍的城主、征讨无国籍的城是人与人的战争。
//! - 国与国之间开战要先宣战、要有理由，没有理由扣风评；只有国君能对别国宣战，封臣不能
//! - 封臣可以直接对无国籍的城开战；无国籍的城主打谁都不用理由，打某国的城就是和这个国家开战
//! - 同一国的封臣之间可以互相打，但要理由；国君可以制止，封臣不停就扣风评，也成为国君征讨他的理由
//! - 国君征讨自己的封臣也是国战，其他封臣都参与
//! - 一方提出讲和（投降、割地、赔款、通行权……），另一方接受才结束；之后若干回合不能再开战

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::command::CommandError;
use crate::config::Config;
use crate::deal::{self, Stake};
use crate::event::Event;
use crate::fief::{self, Grievance};
use crate::state::{CityStatus, GameState, Id};

/// 交战的一方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Party {
    Country { id: Id },
    Person { id: Id },
}

impl Party {
    /// 替这一方拿主意的人：国家是国君。
    pub fn leader(&self, state: &GameState) -> Option<Id> {
        match self {
            Party::Country { id } => state.countries.get(id).map(|c| c.ruler),
            Party::Person { id } => Some(*id),
        }
    }

    pub fn name(&self, state: &GameState) -> String {
        match self {
            Party::Country { id } => state
                .countries
                .get(id)
                .map_or("?".into(), |c| format!("{}国", c.name)),
            Party::Person { id } => state
                .characters
                .get(id)
                .map_or("?".into(), |c| c.name.clone()),
        }
    }
}

/// 一场战争。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct War {
    pub id: Id,
    pub attacker: Party,
    pub defender: Party,
    pub started: u32,
    /// 开战时有没有理由
    pub justified: bool,
    /// 国君下令制止了（同一国的封臣之间的战争）。之后再动手扣风评
    #[serde(default)]
    pub halted: bool,
    /// 国战中国君给封臣定的军事任务：封臣 → 要攻打的城
    #[serde(default)]
    pub tasks: BTreeMap<Id, MilitaryTask>,
    /// 一方提出、对方还没答复的讲和
    #[serde(default)]
    pub peace: Option<PeaceOffer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MilitaryTask {
    pub city: Id,
    /// 封臣的军队打过这座城
    pub done: bool,
}

/// 讲和的条件：提出的人给什么、要什么，和赠与、交易一样。只给不要就是投降。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeaceOffer {
    pub from: Id,
    #[serde(default)]
    pub give: Stake,
    #[serde(default)]
    pub take: Stake,
}

/// 停战：讲和之后双方到第几回合为止（不含）不能再开战。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Truce {
    pub a: Party,
    pub b: Party,
    pub until: u32,
}

impl War {
    /// `who` 站在进攻方。国家一方包括所有国人，但不包括对面那个人（国君征讨封臣时）。
    pub fn on_attacker(&self, state: &GameState, who: Id) -> bool {
        includes(state, self.attacker, self.defender, who)
    }

    pub fn on_defender(&self, state: &GameState, who: Id) -> bool {
        includes(state, self.defender, self.attacker, who)
    }

    /// 两人分属这场战争的两边。
    pub fn opposes(&self, state: &GameState, a: Id, b: Id) -> bool {
        (self.on_attacker(state, a) && self.on_defender(state, b))
            || (self.on_defender(state, a) && self.on_attacker(state, b))
    }

    /// 对面的领头人。
    pub fn counterpart(&self, state: &GameState, who: Id) -> Option<Id> {
        if self.attacker.leader(state) == Some(who) {
            self.defender.leader(state)
        } else if self.defender.leader(state) == Some(who) {
            self.attacker.leader(state)
        } else {
            None
        }
    }

    /// 两个人直接对打的战争。
    pub fn opposes_people(&self, a: Id, b: Id) -> bool {
        let (pa, pb) = (Party::Person { id: a }, Party::Person { id: b });
        (self.attacker == pa && self.defender == pb) || (self.attacker == pb && self.defender == pa)
    }

    pub fn involves_country(&self, country: Id) -> bool {
        [self.attacker, self.defender].contains(&Party::Country { id: country })
    }
}

fn includes(state: &GameState, side: Party, other: Party, who: Id) -> bool {
    match side {
        Party::Person { id } => id == who,
        Party::Country { id } => {
            other != (Party::Person { id: who })
                && state.characters.get(&who).and_then(|c| c.nationality) == Some(id)
        }
    }
}

impl GameState {
    /// 两人是否在交战。
    pub fn at_war(&self, a: Id, b: Id) -> bool {
        a != b && self.wars.values().any(|w| w.opposes(self, a, b))
    }

    /// 两人之间的战争。
    pub fn war_between(&self, a: Id, b: Id) -> Option<Id> {
        self.wars
            .values()
            .find(|w| a != b && w.opposes(self, a, b))
            .map(|w| w.id)
    }

    fn ruler_of(&self, who: Id) -> Option<Id> {
        self.countries
            .values()
            .find(|c| c.ruler == who)
            .map(|c| c.id)
    }

    fn nationality(&self, who: Id) -> Option<Id> {
        self.characters.get(&who).and_then(|c| c.nationality)
    }
}

/// `actor` 对 `target` 开战时两边各是谁。封臣不能对别国开战；打别国的人就是打这个国家。
fn sides(state: &GameState, actor: Id, target: Party) -> Result<(Party, Party), CommandError> {
    let own = state.nationality(actor);
    let ruler_of = state.ruler_of(actor);
    let attacker = match ruler_of {
        Some(country) => Party::Country { id: country },
        None => Party::Person { id: actor },
    };
    let defender = match target {
        Party::Country { id } => {
            if !state.countries.contains_key(&id) {
                return Err(CommandError::NoSuchCountry(id));
            }
            if own == Some(id) {
                return Err(CommandError::OwnCountry);
            }
            target
        }
        Party::Person { id } => {
            if !state.characters.contains_key(&id) {
                return Err(CommandError::NoSuchCharacter(id));
            }
            if id == actor {
                return Err(CommandError::OwnCountry);
            }
            match state.nationality(id) {
                // 别国的人：就是和这个国家开战
                Some(country) if own != Some(country) => Party::Country { id: country },
                _ => {
                    if state.ruler_of(id).is_some() {
                        Party::Country {
                            id: state.ruler_of(id).expect("checked"),
                        }
                    } else {
                        target
                    }
                }
            }
        }
    };
    // 有国籍、不是国君的人（封臣）不能对别国开战
    if let Party::Country { id } = defender
        && ruler_of.is_none()
        && own.is_some()
        && own != Some(id)
    {
        return Err(CommandError::VassalCannotDeclare);
    }
    Ok((attacker, defender))
}

/// 开战的理由，由系统判定。现在只有封君征讨封臣的理由，其他理由以后补。
pub fn reasons(state: &GameState, config: &Config, actor: Id, defender: Party) -> Vec<Grievance> {
    let Party::Person { id: vassal } = defender else {
        return Vec::new();
    };
    let mut found: Vec<Grievance> = Vec::new();
    for city in state
        .cities
        .values()
        .filter(|c| c.owner == Some(vassal) && c.liege == Some(actor))
    {
        for g in fief::grievances(state, config, city) {
            if !found.contains(&g) {
                found.push(g);
            }
        }
    }
    found
}

/// 两边都是国家里的人时开战要理由；牵涉无国籍的人就不用。
fn needs_reason(state: &GameState, attacker: Party, defender: Party) -> bool {
    let in_country = |p: Party| match p {
        Party::Country { .. } => true,
        Party::Person { id } => state.nationality(id).is_some(),
    };
    in_country(attacker) && in_country(defender)
}

/// 宣战。没有理由时扣风评，返回新战争的 id。
pub fn declare(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    target: Party,
    events: &mut Vec<Event>,
) -> Result<Id, CommandError> {
    let (attacker, defender) = sides(state, actor, target)?;
    if state.wars.values().any(|w| {
        [w.attacker, w.defender] == [attacker, defender]
            || [w.attacker, w.defender] == [defender, attacker]
    }) {
        return Err(CommandError::AlreadyAtWar);
    }
    if state.truces.iter().any(|t| {
        state.turn < t.until
            && ([t.a, t.b] == [attacker, defender] || [t.a, t.b] == [defender, attacker])
    }) {
        return Err(CommandError::Truce);
    }
    let found = reasons(state, config, actor, defender);
    let justified = !needs_reason(state, attacker, defender) || !found.is_empty();
    let id = state.alloc_id();
    let names = (attacker.name(state), defender.name(state));
    if !justified && let Some(c) = state.characters.get_mut(&actor) {
        c.reputation -= config.war.unjustified_penalty;
    }
    let why = if found.is_empty() {
        if justified {
            String::new()
        } else {
            "，没有理由，风评受损".into()
        }
    } else {
        let list: Vec<String> = found.iter().map(|g| g.describe()).collect();
        format!("，理由：{}", list.join("、"))
    };
    events.push(Event::public(format!("{}向{}宣战{why}", names.0, names.1)));
    state.wars.insert(
        id,
        War {
            id,
            attacker,
            defender,
            started: state.turn,
            justified,
            halted: false,
            tasks: BTreeMap::new(),
            peace: None,
        },
    );
    refresh_national_war(state);
    Ok(id)
}

/// 国家在打国与国的战争（国战）时，国内所有的城对本国的人开放。
pub fn refresh_national_war(state: &mut GameState) {
    let ids: Vec<Id> = state.countries.keys().copied().collect();
    for id in ids {
        let at_war = state.wars.values().any(|w| w.involves_country(id));
        state.countries.get_mut(&id).expect("listed").national_war = at_war;
    }
}

/// 国君制止本国两名封臣之间的战争。
pub fn halt(state: &mut GameState, actor: Id, war_id: Id) -> Result<(), CommandError> {
    let war = state
        .wars
        .get(&war_id)
        .ok_or(CommandError::NoSuchWar(war_id))?;
    let country = state.ruler_of(actor).ok_or(CommandError::NotRuler)?;
    let ours =
        |p: Party| matches!(p, Party::Person { id } if state.nationality(id) == Some(country));
    if !(ours(war.attacker) && ours(war.defender)) {
        return Err(CommandError::NotRuler);
    }
    state.wars.get_mut(&war_id).expect("checked").halted = true;
    Ok(())
}

/// 国战中国君给封臣定军事任务：攻打哪座城。
pub fn assign_task(
    state: &mut GameState,
    actor: Id,
    war_id: Id,
    vassal: Id,
    city: Id,
) -> Result<(), CommandError> {
    let war = state
        .wars
        .get(&war_id)
        .ok_or(CommandError::NoSuchWar(war_id))?;
    let country = state.ruler_of(actor).ok_or(CommandError::NotRuler)?;
    let ours = Party::Country { id: country };
    let enemy = if war.attacker == ours {
        war.defender
    } else if war.defender == ours {
        war.attacker
    } else {
        return Err(CommandError::NotRuler);
    };
    if vassal == actor || state.nationality(vassal) != Some(country) {
        return Err(CommandError::NotYourVassal(vassal));
    }
    let target = state
        .cities
        .get(&city)
        .ok_or(CommandError::NoSuchCity(city))?;
    let holder = target.holder();
    let theirs = holder.is_some_and(|h| includes(state, enemy, ours, h));
    if !theirs {
        return Err(CommandError::NotEnemyCity(city));
    }
    state
        .wars
        .get_mut(&war_id)
        .expect("checked")
        .tasks
        .insert(vassal, MilitaryTask { city, done: false });
    Ok(())
}

/// 有人攻打了一座城或一支军队：国战任务算完成；国君制止后还动手的扣风评，成为征讨的理由。
pub(crate) fn note_attack(
    state: &mut GameState,
    config: &Config,
    attacker: Id,
    target: Id,
    city: Option<Id>,
    events: &mut Vec<Event>,
) {
    let Some(war_id) = state.war_between(attacker, target) else {
        return;
    };
    let turn = state.turn;
    let war = state.wars.get_mut(&war_id).expect("found");
    if let (Some(city), Some(task)) = (city, war.tasks.get_mut(&attacker))
        && task.city == city
    {
        task.done = true;
    }
    if war.halted
        && let Some(c) = state.characters.get_mut(&attacker)
    {
        c.reputation -= config.war.defy_penalty;
        c.defied_turn = Some(turn);
        events.push(Event::public(format!("{}不听国君制止，风评受损", c.name)));
    }
}

/// 提出讲和：`give` 是自己交出的，`take` 是要对方交出的。只给不要就是投降。
pub fn propose_peace(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    war_id: Id,
    give: &Stake,
    take: &Stake,
) -> Result<(), CommandError> {
    let war = state
        .wars
        .get(&war_id)
        .ok_or(CommandError::NoSuchWar(war_id))?;
    let other = war
        .counterpart(state, actor)
        .ok_or(CommandError::NotWarLeader(war_id))?;
    deal::check_stake_with(state, config, actor, give, true)?;
    deal::check_stake_with(state, config, other, take, true)?;
    state.wars.get_mut(&war_id).expect("checked").peace = Some(PeaceOffer {
        from: actor,
        give: give.clone(),
        take: take.clone(),
    });
    Ok(())
}

/// 接受讲和：交割条件，战争结束。
pub fn accept_peace(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    war_id: Id,
    events: &mut Vec<Event>,
) -> Result<(), CommandError> {
    let war = state
        .wars
        .get(&war_id)
        .ok_or(CommandError::NoSuchWar(war_id))?;
    let offer = war
        .peace
        .clone()
        .ok_or(CommandError::NoPeaceOffer(war_id))?;
    if war.counterpart(state, offer.from) != Some(actor) {
        return Err(CommandError::NoPeaceOffer(war_id));
    }
    deal::settle_peace(state, config, offer.from, actor, &offer.give, &offer.take)?;
    end(state, config, war_id, events);
    Ok(())
}

pub fn decline_peace(state: &mut GameState, actor: Id, war_id: Id) -> Result<(), CommandError> {
    let war = state
        .wars
        .get(&war_id)
        .ok_or(CommandError::NoSuchWar(war_id))?;
    let offer = war
        .peace
        .as_ref()
        .ok_or(CommandError::NoPeaceOffer(war_id))?;
    if war.counterpart(state, offer.from) != Some(actor) && offer.from != actor {
        return Err(CommandError::NoPeaceOffer(war_id));
    }
    state.wars.get_mut(&war_id).expect("checked").peace = None;
    Ok(())
}

/// 战争结束：没割让的被攻占的城还给原主；国战任务没完成的封臣扣风评；双方停战若干回合。
pub(crate) fn end(state: &mut GameState, config: &Config, war_id: Id, events: &mut Vec<Event>) {
    let Some(war) = state.wars.remove(&war_id) else {
        return;
    };
    let returned: Vec<Id> = state
        .cities
        .values()
        .filter(|c| {
            c.status == CityStatus::Occupied
                && c.occupier
                    .is_some_and(|o| c.owner.is_some_and(|w| war.opposes(state, o, w)))
        })
        .map(|c| c.id)
        .collect();
    for id in returned {
        let owner = state.cities[&id].owner.expect("checked");
        let manages_other = state
            .cities
            .values()
            .any(|c| c.id != id && c.manager == Some(owner));
        let city = state.cities.get_mut(&id).expect("listed");
        city.status = CityStatus::Ruled;
        city.occupier = None;
        if manages_other {
            city.manager = None;
        } else {
            city.change_manager(owner, config);
        }
        events.push(Event::public(format!("{}还给了原主", city.name)));
    }
    let turn = state.turn;
    for (vassal, task) in &war.tasks {
        if task.done {
            continue;
        }
        if let Some(c) = state.characters.get_mut(vassal) {
            c.reputation -= config.war.task_penalty;
            c.defied_turn = Some(turn);
            events.push(Event::public(format!(
                "{}没有执行国君的军事任务，风评受损",
                c.name
            )));
        }
    }
    state.truces.push(Truce {
        a: war.attacker,
        b: war.defender,
        until: state.turn + config.war.truce_turns,
    });
    events.push(Event::public(format!(
        "{}与{}讲和，{} 回合内不能再开战",
        war.attacker.name(state),
        war.defender.name(state),
        config.war.truce_turns
    )));
    refresh_national_war(state);
}

/// 回合结束：没答复的讲和作废，过期的停战去掉，灭亡的国家的战争结束。
pub(crate) fn end_turn(state: &mut GameState) {
    for war in state.wars.values_mut() {
        war.peace = None;
    }
    let next = state.turn + 1;
    state.truces.retain(|t| t.until > next);
    let gone: Vec<Id> = state
        .wars
        .values()
        .filter(|w| {
            [w.attacker, w.defender]
                .iter()
                .any(|p| matches!(p, Party::Country { id } if !state.countries.contains_key(id)))
        })
        .map(|w| w.id)
        .collect();
    for id in gone {
        state.wars.remove(&id);
    }
    refresh_national_war(state);
}
