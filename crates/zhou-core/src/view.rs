//! 发给每个玩家的报告：从全局的回合报告里只挑出他看得到的部分。服务器只把这一份发给他，
//! 不靠客户端隐藏。

use crate::fx::Fx;
use serde::Serialize;

use crate::config::Config;
use crate::deal::Deal;
use crate::event::Event;
use crate::fief::Grievance;
use crate::map::Hex;
use crate::state::{Country, GameState, Id, Tribute};
use crate::turn::{ArmyReport, CellReport, CharacterReport, CityReport, TurnReport};
use crate::vision::{ArmySighting, CitySighting};
use crate::war::War;

#[derive(Debug, Clone, Serialize)]
pub struct PlayerReport {
    /// 这份报告是给谁的
    pub me: Id,
    pub turn: u32,
    pub year: u32,
    pub month: u32,
    pub season: String,
    /// 他看得到的消息：当事人的、视野里发生的、公开的
    pub events: Vec<Event>,
    /// 眼下看得到的格子
    pub vision: Vec<Hex>,
    /// 已探索地形与王室都城所在格；未探索地形不下发
    pub map: Vec<CellReport>,
    /// 自己拥有、管理、攻占的城，看得到全部内情
    pub cities: Vec<CityReport>,
    /// 别人的城：见过的外观。`turn` 是最后一次看到的回合
    pub foreign_cities: Vec<ForeignCity>,
    /// 自己统领的军队
    pub armies: Vec<ArmyReport>,
    /// 别人的军队：视野里的，和离开视野前最后一次看到的位置、时间
    pub foreign_armies: Vec<ForeignArmy>,
    /// 自己的全部情况
    pub character: Option<CharacterReport>,
    /// 其他人公开的情况
    pub characters: Vec<PublicCharacter>,
    pub countries: Vec<Country>,
    /// 战争是公开的
    pub wars: Vec<War>,
    /// 别人给他的、还没答复的任命和封授
    pub offers: Vec<Offer>,
    /// 他提出的和别人向他提出的交易，当回合有效
    pub deals: Vec<Deal>,
}

/// 一份等他答复的任命或封授。
#[derive(Debug, Clone, Serialize)]
pub struct Offer {
    /// `management` 管理权任命，`fief` 封授
    pub kind: String,
    pub city: Id,
    pub city_name: String,
    /// 提出的人：城的所有者
    pub from: Option<Id>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignCity {
    /// 未探索到的王室都城，仅为公共地标。
    pub public_landmark: bool,
    #[serde(flatten)]
    pub sighting: CitySighting,
    /// 眼下在视野里
    pub visible: bool,
    /// 自己是这座城的封君：看得到纳贡和征讨的理由
    pub fief: Option<FiefView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FiefView {
    pub tribute: Option<Tribute>,
    pub tribute_excessive: bool,
    pub grievances: Vec<Grievance>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignArmy {
    #[serde(flatten)]
    pub sighting: ArmySighting,
    /// 眼下在视野里；不在时 pos、turn 是最后一次看到的
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicCharacter {
    pub id: Id,
    pub name: String,
    pub rank: String,
    pub reputation: Fx,
    pub nationality: Option<Id>,
}

/// 从回合报告里挑出 `who` 看得到的部分。在回合结算之后、下一回合的指令之前调用。
pub fn player_report(
    state: &GameState,
    config: &Config,
    report: &TurnReport,
    who: Id,
) -> PlayerReport {
    let vision = crate::vision::vision(state, config, who);
    let intel = state.intel.get(&who).cloned().unwrap_or_default();
    let mine = |id: Id| {
        state
            .cities
            .get(&id)
            .is_some_and(|c| c.insiders().contains(&who))
    };
    let capitals = crate::vision::public_capitals(state, config);
    let known = |id: Id| mine(id) || intel.cities.contains_key(&id) || capitals.contains(&id);
    let mut explored = intel.explored.clone();
    explored.extend(vision.iter().copied());
    if intel.explored.is_empty() {
        for city in intel.cities.values() {
            explored.extend(city.tiles.iter().copied());
        }
        explored.extend(intel.armies.values().map(|a| a.pos));
    }
    let mut sightings = intel.cities.clone();
    for id in &capitals {
        if !sightings.contains_key(id)
            && !mine(*id)
            && let Some(city) = state.cities.get(id)
        {
            let mut landmark = crate::vision::glimpse(state, config, city);
            landmark.tiles = vec![city.seat];
            landmark.size.clear();
            landmark.wall = false;
            landmark.wall_broken = false;
            sightings.insert(*id, landmark);
        }
    }
    let public_seats: std::collections::BTreeSet<_> = capitals
        .iter()
        .filter_map(|id| state.cities.get(id).map(|c| c.seat))
        .collect();
    let foreign_cities = sightings
        .values()
        .filter(|c| !mine(c.id))
        .map(|c| {
            let full = report.cities.iter().find(|r| r.id == c.id);
            let mut sighting = c.clone();
            sighting
                .tiles
                .retain(|p| explored.contains(p) || (capitals.contains(&c.id) && *p == c.seat));
            ForeignCity {
                public_landmark: capitals.contains(&c.id) && !intel.cities.contains_key(&c.id),
                visible: c.tiles.iter().any(|p| vision.contains(p)),
                fief: full.filter(|r| r.liege == Some(who)).map(|r| FiefView {
                    tribute: r.tribute.clone(),
                    tribute_excessive: r.tribute_excessive,
                    grievances: r.grievances.clone(),
                }),
                sighting,
            }
        })
        .collect();
    let foreign_armies = intel
        .armies
        .values()
        .map(|s| ForeignArmy {
            visible: vision.contains(&s.pos)
                && state.armies.get(&s.id).is_some_and(|a| a.pos == s.pos),
            sighting: s.clone(),
        })
        .collect();
    PlayerReport {
        me: who,
        turn: report.turn,
        year: report.year,
        month: report.month,
        season: report.season.clone(),
        events: report
            .events
            .iter()
            .filter(|e| e.seen_by(who, &vision))
            .cloned()
            .collect(),
        vision: vision.iter().copied().collect(),
        map: report
            .map
            .iter()
            .filter(|c| explored.contains(&c.pos) || public_seats.contains(&c.pos))
            .map(|c| CellReport {
                city: c.city.filter(|id| known(*id)),
                ..c.clone()
            })
            .collect(),
        cities: report
            .cities
            .iter()
            .filter(|c| mine(c.id))
            .cloned()
            .collect(),
        foreign_cities,
        armies: report
            .armies
            .iter()
            .filter(|a| a.commander == who)
            .cloned()
            .collect(),
        foreign_armies,
        character: report.characters.iter().find(|c| c.id == who).cloned(),
        characters: report
            .characters
            .iter()
            .filter(|c| c.id != who)
            .map(|c| PublicCharacter {
                id: c.id,
                name: c.name.clone(),
                rank: c.rank.clone(),
                reputation: c.reputation,
                nationality: c.nationality,
            })
            .collect(),
        countries: report.countries.clone(),
        wars: report.wars.clone(),
        offers: state
            .cities
            .values()
            .flat_map(|c| {
                let offer = |kind: &str| Offer {
                    kind: kind.into(),
                    city: c.id,
                    city_name: c.name.clone(),
                    from: c.owner,
                };
                [
                    (c.manager_offer == Some(who)).then(|| offer("management")),
                    (c.fief_offer == Some(who)).then(|| offer("fief")),
                ]
            })
            .flatten()
            .collect(),
        deals: state
            .deals
            .values()
            .filter(|d| d.from == who || d.to == who)
            .cloned()
            .collect(),
    }
}
