use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::option::Option;
use chrono::{Local, DateTime};

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum OrderCondition {
    Unconditional,
    Stop { stop: i32 },
}

#[derive(Debug, Eq, Copy, Clone)]
pub struct Order {
    pub id: i32,
    pub buy: bool,
    pub volume: i32,
    pub limit_price: Option<i32>,
    pub created_at: DateTime<Local>,
    pub filled_at: Option<DateTime<Local>>,
    pub condition: OrderCondition,
}

impl Order {
    pub fn new(
        id: i32,
        buy: bool,
        volume: i32,
        limit_price: Option<i32>,
        condition: OrderCondition,
    ) -> Order {
        Order {
            id,
            buy,
            limit_price,
            volume,
            created_at: Local::now(),
            filled_at: None,
            condition,
        }
    }

    #[inline]
    pub fn fill(&mut self) {
        self.filled_at = Some(Local::now());
    }

    #[inline]
    pub fn is_active_with(&self, head_order: &Order) -> bool {
        let ls = self.trade_direction();
        match self.condition {
            OrderCondition::Unconditional => match head_order.condition {
                OrderCondition::Unconditional => {
                    self.limit_price.map(|x| x * ls) < head_order.limit_price.map(|x| x * ls)
                }
                OrderCondition::Stop { .. } => true,
            },
            _ => false,
        }
    }

    #[inline]
    pub fn split_at_volume(&mut self, vol_to_split: i32) -> Order {
        self.volume -= vol_to_split;
        Order {
            volume: vol_to_split,
            ..*self
        }
    }

    #[inline]
    pub fn trade_direction(&self) -> i32 {
        2 * (self.buy as i32) - 1
    }
}

impl PartialOrd for Order {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let ls_self = self.trade_direction();
        let ls_other = other.trade_direction();

        match (self.condition, other.condition) {
            (OrderCondition::Unconditional, OrderCondition::Unconditional) => {
                (self.limit_price.map(|x| x * ls_self), self.volume)
                    .partial_cmp(&(other.limit_price.map(|x| x * ls_other), other.volume))
            }
            (OrderCondition::Unconditional, OrderCondition::Stop { stop }) => {
                (self.limit_price.map(|x| x * ls_self), self.volume)
                    .partial_cmp(&(Some(stop * ls_other), other.volume))
            }
            (OrderCondition::Stop { stop }, OrderCondition::Unconditional) => {
                (Some(stop * ls_self), other.volume)
                    .partial_cmp(&(other.limit_price.map(|x| x * ls_other), other.volume))
            }
            (OrderCondition::Stop { stop: s }, OrderCondition::Stop { stop: o }) => (
                s * ls_self,
                self.volume,
                self.limit_price.map(|x| x * ls_other),
            )
                .partial_cmp(&(
                    o * ls_other,
                    other.volume,
                    other.limit_price.map(|x| x * ls_self),
                )),
        }
    }
}

impl Ord for Order {
    fn cmp(&self, other: &Self) -> Ordering {
        let ls_self = self.trade_direction();
        let ls_other = other.trade_direction();

        match (self.condition, other.condition) {
            (OrderCondition::Unconditional, OrderCondition::Unconditional) => {
                (self.limit_price.map(|x| x * ls_self), self.volume)
                    .cmp(&(other.limit_price.map(|x| x * ls_other), other.volume))
            }
            (OrderCondition::Unconditional, OrderCondition::Stop { stop }) => {
                (self.limit_price.map(|x| x * ls_self), self.volume)
                    .cmp(&(Some(stop * ls_other), other.volume))
            }
            (OrderCondition::Stop { stop }, OrderCondition::Unconditional) => {
                (Some(stop * ls_self), other.volume)
                    .cmp(&(other.limit_price.map(|x| x * ls_other), other.volume))
            }
            (OrderCondition::Stop { stop: s }, OrderCondition::Stop { stop: o }) => (
                s * ls_self,
                self.volume,
                self.limit_price.map(|x| x * ls_other),
            )
                .cmp(&(
                    o * ls_other,
                    other.volume,
                    other.limit_price.map(|x| x * ls_self),
                )),
        }
    }
}

impl PartialEq for Order {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Hash for Order {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.id.hash(h)
    }
}
