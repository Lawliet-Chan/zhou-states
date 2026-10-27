//! 回合中发生的事。每条消息记着谁能看到：当事人一定能看到；发生在某一格的事，
//! 这一格在视野里的人也能看到；宣战、讲和、国家灭亡这类大事所有人都能看到。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::map::Hex;
use crate::state::{City, Id};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub text: String,
    /// 当事人，一定能看到
    #[serde(default)]
    pub to: Vec<Id>,
    /// 发生的地点，这一格在视野里的人也能看到
    #[serde(default)]
    pub at: Option<Hex>,
    /// 所有人都能看到
    #[serde(default)]
    pub public: bool,
}

impl Event {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            to: Vec::new(),
            at: None,
            public: false,
        }
    }

    pub fn public(text: impl Into<String>) -> Self {
        Self {
            public: true,
            ..Self::new(text)
        }
    }

    /// 加上当事人。
    pub fn to(mut self, ids: impl IntoIterator<Item = Id>) -> Self {
        for id in ids {
            if !self.to.contains(&id) {
                self.to.push(id);
            }
        }
        self
    }

    /// 城里的事：城的所有者、管理者、占领者是当事人。
    pub fn city(text: impl Into<String>, city: &City) -> Self {
        Self::new(text).to(city.insiders())
    }

    pub fn at(mut self, pos: Hex) -> Self {
        self.at = Some(pos);
        self
    }

    /// `who` 能不能看到这条消息，`vision` 是他眼下看得到的格子。
    pub fn seen_by(&self, who: Id, vision: &BTreeSet<Hex>) -> bool {
        self.public || self.to.contains(&who) || self.at.is_some_and(|p| vision.contains(&p))
    }
}
