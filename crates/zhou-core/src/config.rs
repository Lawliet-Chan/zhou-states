use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::inventory::Inventory;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("读取配置文件失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("配置文件格式错误：{0}")]
    Parse(#[from] toml::de::Error),
    #[error("配置内容错误：{0}")]
    Invalid(String),
}

/// 全部数值配置，对应 config/balance.toml。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub seasons: Vec<String>,
    pub population: PopulationConfig,
    pub happiness: HappinessConfig,
    pub shi: ShiConfig,
    pub economy: EconomyConfig,
    pub sacrifice: SacrificeConfig,
    pub ritual: RitualConfig,
    pub goods: BTreeMap<String, GoodDef>,
    pub skills: BTreeMap<String, SkillDef>,
    pub recipes: BTreeMap<String, RecipeDef>,
    pub buildings: BTreeMap<String, BuildingDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PopulationConfig {
    pub per_people: f64,
    pub max_skills: usize,
    pub specialization: Vec<f64>,
    pub growth_rate: f64,
    pub growth_min_happiness: f64,
    pub emigration_threshold: f64,
    pub emigration_rate: f64,
    pub starvation_rate: f64,
    pub treasury_covers_shortfall: bool,
    pub consumption: Vec<ConsumptionRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsumptionRule {
    pub name: String,
    pub goods: Vec<String>,
    pub amount: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HappinessConfig {
    pub max: f64,
    pub target: f64,
    pub drift_rate: f64,
    pub guan_penalty: f64,
    pub tax_penalty: f64,
    pub hunger_penalty: f64,
    pub sacrifice_bonus: f64,
    pub wealth_bonus: f64,
    pub wealth_full_per_person: f64,
    pub min_output_mult: f64,
    pub max_output_mult: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShiConfig {
    pub skill: String,
    pub base_capacity: f64,
    pub new_capacity_ratio: f64,
    pub ramp_turns: u32,
    pub loyalty_min_mult: f64,
    pub loyalty_max: f64,
    pub initial_loyalty: f64,
    pub unpaid_loyalty_penalty: f64,
    pub paid_loyalty_gain: f64,
    pub upkeep: Inventory,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EconomyConfig {
    pub guan_efficiency: f64,
    pub default_tax_rate: f64,
    pub max_tax_rate: f64,
    pub absentee_mult: f64,
    pub occupied_mult: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SacrificeConfig {
    pub vessel_goods: Vec<String>,
    pub culture_per_vessel: f64,
    pub culture_max: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RitualConfig {
    pub ding_good: String,
    pub excess_reputation_penalty: f64,
    pub ding_limit: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoodDef {
    pub name: String,
    /// 价值，用于折算民间财富的多少
    #[serde(default = "default_value")]
    pub value: f64,
}

fn default_value() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillDef {
    pub name: String,
    pub default_recipe: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeDef {
    pub name: String,
    pub skill: String,
    pub resource: Option<String>,
    pub facility: Option<String>,
    #[serde(default)]
    pub inputs: Inventory,
    pub outputs: Inventory,
    pub season_mult: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildingDef {
    pub name: String,
    pub skill: Option<String>,
    pub labor: f64,
    #[serde(default)]
    pub materials: Inventory,
    pub season_mult: Vec<f64>,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        Self::from_toml(&std::fs::read_to_string(path)?)
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    /// 技能槽中有 `skill_count` 项技能时的专精度。
    pub fn specialization(&self, skill_count: usize) -> f64 {
        let table = &self.population.specialization;
        table[skill_count.clamp(1, table.len()) - 1]
    }

    /// 愉悦度对产出的倍率。
    pub fn happiness_output_mult(&self, happiness: f64) -> f64 {
        let h = &self.happiness;
        let t = (happiness / h.max).clamp(0.0, 1.0);
        h.min_output_mult + (h.max_output_mult - h.min_output_mult) * t
    }

    /// 一堆物品按价值折算的总量。
    pub fn value_of(&self, inventory: &Inventory) -> f64 {
        inventory.iter().map(|(good, amount)| self.goods.get(good).map_or(0.0, |g| g.value) * amount).sum()
    }

    pub fn good_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.goods.get(id).map_or(id, |g| g.name.as_str())
    }

    pub fn skill_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.skills.get(id).map_or(id, |s| s.name.as_str())
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |msg: String| Err(ConfigError::Invalid(msg));
        let seasons = self.seasons.len();
        if seasons == 0 {
            return invalid("seasons 不能为空".into());
        }
        if self.population.specialization.len() != self.population.max_skills {
            return invalid("population.specialization 的长度必须等于 max_skills".into());
        }
        if !self.skills.contains_key(&self.shi.skill) {
            return invalid(format!("shi.skill「{}」不在 skills 中", self.shi.skill));
        }
        let check_goods = |inv: &Inventory, owner: &str| -> Result<(), ConfigError> {
            for good in inv.0.keys() {
                if !self.goods.contains_key(good) {
                    return Err(ConfigError::Invalid(format!("{owner} 引用了不存在的物品「{good}」")));
                }
            }
            Ok(())
        };
        check_goods(&self.shi.upkeep, "shi.upkeep")?;
        for rule in &self.population.consumption {
            for good in &rule.goods {
                if !self.goods.contains_key(good) {
                    return invalid(format!("消耗「{}」引用了不存在的物品「{good}」", rule.name));
                }
            }
        }
        for good in self.sacrifice.vessel_goods.iter().chain([&self.ritual.ding_good]) {
            if !self.goods.contains_key(good) {
                return invalid(format!("礼器「{good}」不在 goods 中"));
            }
        }
        for (id, skill) in &self.skills {
            if let Some(recipe) = &skill.default_recipe
                && !self.recipes.contains_key(recipe)
            {
                return invalid(format!("技能「{id}」的 default_recipe「{recipe}」不存在"));
            }
        }
        for (id, recipe) in &self.recipes {
            if !self.skills.contains_key(&recipe.skill) {
                return invalid(format!("配方「{id}」的技能「{}」不存在", recipe.skill));
            }
            if let Some(facility) = &recipe.facility
                && !self.buildings.contains_key(facility)
            {
                return invalid(format!("配方「{id}」的设施「{facility}」不存在"));
            }
            if recipe.season_mult.len() != seasons {
                return invalid(format!("配方「{id}」的 season_mult 长度必须等于季节数"));
            }
            check_goods(&recipe.inputs, &format!("配方「{id}」"))?;
            check_goods(&recipe.outputs, &format!("配方「{id}」"))?;
        }
        for (id, building) in &self.buildings {
            if let Some(skill) = &building.skill
                && !self.skills.contains_key(skill)
            {
                return invalid(format!("建筑「{id}」的技能「{skill}」不存在"));
            }
            if building.season_mult.len() != seasons {
                return invalid(format!("建筑「{id}」的 season_mult 长度必须等于季节数"));
            }
            check_goods(&building.materials, &format!("建筑「{id}」"))?;
        }
        Ok(())
    }
}
