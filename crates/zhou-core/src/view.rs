//! 发给每个玩家的报告：从全局的回合报告里只挑出他看得到的部分。服务器只把这一份发给他，
//! 不靠客户端隐藏。

use serde::Serialize;

use crate::config::Config;
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
    /// 整张地图的地形都公开；格子属于哪座城，只有见过那座城才知道
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
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignCity {
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
    pub reputation: f64,
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
    let known = |id: Id| mine(id) || intel.cities.contains_key(&id);
    let foreign_cities = intel
        .cities
        .values()
        .filter(|c| !mine(c.id))
        .map(|c| {
            let full = report.cities.iter().find(|r| r.id == c.id);
            ForeignCity {
                visible: c.tiles.iter().any(|p| vision.contains(p)),
                fief: full.filter(|r| r.liege == Some(who)).map(|r| FiefView {
                    tribute: r.tribute.clone(),
                    tribute_excessive: r.tribute_excessive,
                    grievances: r.grievances.clone(),
                }),
                sighting: c.clone(),
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
    }
}
