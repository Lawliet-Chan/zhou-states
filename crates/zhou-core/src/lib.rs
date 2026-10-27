//! 周代列国的规则核心：只包含游戏状态和规则，不依赖任何引擎或网络。

pub mod army;
pub mod combat;
pub mod command;
pub mod config;
pub mod deal;
pub mod event;
pub mod fief;
pub mod inventory;
pub mod map;
pub mod scenario;
pub mod state;
pub mod supply;
pub mod turn;
pub mod view;
pub mod vision;
pub mod war;

pub use army::{Army, Squad, SquadOrder};
pub use combat::{Appointee, PlunderTarget};
pub use command::{Command, CommandError, apply};
pub use config::{Config, Pool};
pub use deal::{Deal, Passage, Stake};
pub use event::Event;
pub use inventory::Inventory;
pub use map::{Cell, Hex, Map};
pub use scenario::load_scenario;
pub use state::{
    Building, Character, Country, GameState, Id, JobSlot, Learning, Mode, Quota, Task, Tile,
    Tribute,
};
pub use supply::SupplyLine;
pub use turn::{TurnReport, end_turn};
pub use view::{PlayerReport, player_report};
pub use war::{Party, War};
