//! 封授与纳贡：判断纳贡是否过火、封君征讨封臣的理由。

use serde::Serialize;

use crate::config::Config;
use crate::state::{City, GameState};

/// 判断纳贡是否过火时参照的府库一年总产出价值：有去年的就用去年的，
/// 否则用今年到目前为止的折算成一年。`months` 是今年已经结算过的回合数。
pub fn yearly_income_value(city: &City, config: &Config, months: u32) -> f64 {
    match &city.last_income {
        Some(last) => config.value_of(last),
        None if months == 0 => 0.0,
        None => config.value_of(&city.income) * config.turns_per_year() as f64 / months as f64,
    }
}

/// 封君指定的纳贡过火了多少：要的总价值是上限的几倍再减 1。没有指定纳贡或者没有过火时返回 None。
pub fn overreach(city: &City, config: &Config, months: u32) -> Option<f64> {
    let tribute = city.tribute.as_ref()?;
    let demanded = config.value_of(&tribute.goods);
    let limit = yearly_income_value(city, config, months) * config.fief.excessive_ratio;
    if demanded <= limit + 1e-9 {
        return None;
    }
    // 府库没有产出时，要什么都算过火，按最重的算
    Some(if limit > 0.0 { demanded / limit - 1.0 } else { f64::INFINITY })
}

/// 封君征讨封臣的理由。封地不能收回，这些只能作为征讨的理由，成立的越多，征讨时战斗力越强。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Grievance {
    /// 封臣风评太低
    LowReputation,
    /// 封臣最近一年内僭越过
    Usurpation,
    /// 合理的纳贡连续几年没交够
    TributeShortfall { years: u32 },
}

impl Grievance {
    pub fn describe(&self) -> String {
        match self {
            Grievance::LowReputation => "风评太低".into(),
            Grievance::Usurpation => "僭越".into(),
            Grievance::TributeShortfall { years } => format!("连续 {years} 年纳贡不足"),
        }
    }
}

/// 封君对这座封地的封臣有哪些征讨的理由。不是封地时为空。
pub fn grievances(state: &GameState, config: &Config, city: &City) -> Vec<Grievance> {
    let (Some(_), Some(vassal)) = (city.liege, city.owner.and_then(|o| state.characters.get(&o))) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    if vassal.reputation < config.fief.low_reputation {
        found.push(Grievance::LowReputation);
    }
    if vassal.usurped_turn.is_some_and(|t| state.turn < t + config.turns_per_year()) {
        found.push(Grievance::Usurpation);
    }
    if city.tribute_shortfall_years >= config.fief.shortfall_years {
        found.push(Grievance::TributeShortfall { years: city.tribute_shortfall_years });
    }
    found
}
