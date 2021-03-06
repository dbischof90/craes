use chrono::{DateTime, Local};
use ordered_float;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::option::Option;

/// A generic order type.
#[derive(Hash, Debug, Eq, PartialEq, Copy, Clone)]
pub enum Order {
    LimitMarket(UnconditionalOrder),
    StopLimit(ConditionalOrder),
}

/// A complementary structure. Used to reconstruct a conditional order after converting it to an
/// unconditional order.
#[derive(Debug)]
pub struct OrderComplement {
    pub trigger_price: ordered_float::OrderedFloat<f32>,
    pub condition: ConditionalType,
}

/// A type compositor for conditional orders. Specializes order type and allows specific treatment
/// in resolution.
#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub enum ConditionalType {
    StopLoss,
    StopAndReverse,
}

/// Unconditional orders. These orders can be either limit orders
/// or market orders, depending whether they have a limit price or not.
#[derive(Eq, Debug, Copy, Clone)]
pub struct UnconditionalOrder {
    pub id: u32,
    pub buy: bool,
    pub volume: u32,
    pub limit_price: Option<ordered_float::OrderedFloat<f32>>,
    pub created_at: DateTime<Local>,
    pub filled_at: Option<DateTime<Local>>,
}

/// Conditional orders. This order type represents stop market and
/// limit orders which are executed once a condition is met. They interrupt the
/// ordinary trade execution flow of the limit order and are handled with priority.
#[derive(Eq, Debug, Copy, Clone)]
pub struct ConditionalOrder {
    pub id: u32,
    pub buy: bool,
    pub volume: u32,
    pub limit_price: Option<ordered_float::OrderedFloat<f32>>,
    pub trigger_price: ordered_float::OrderedFloat<f32>,
    pub condition: ConditionalType,
    pub created_at: DateTime<Local>,
    pub filled_at: Option<DateTime<Local>>,
}

impl ConditionalOrder {
    pub fn new(
        id: u32,
        buy: bool,
        volume: u32,
        limit_price: Option<f32>,
        condition: ConditionalType,
        trigger_price: f32,
    ) -> ConditionalOrder {
        ConditionalOrder {
            id,
            buy,
            limit_price: limit_price.map(ordered_float::OrderedFloat::from),
            condition,
            trigger_price: ordered_float::OrderedFloat(trigger_price),
            volume,
            created_at: Local::now(),
            filled_at: None,
        }
    }

    /// Becomes 1 and -1 for a buy and sell order.
    #[inline]
    pub fn trade_direction(&self) -> f32 {
        (2 * (self.buy as u8) - 1) as f32
    }
}

impl PartialEq for ConditionalOrder {
    fn eq(&self, other: &Self) -> bool {
        (self.id, self.volume) == (other.id, other.volume)
    }
}

impl PartialOrd for ConditionalOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let ls_self = self.trade_direction();
        let ls_other = other.trade_direction();

        (
            self.trigger_price.into_inner() * ls_self,
            self.volume,
            self.limit_price.map(|p| p.into_inner() * ls_other),
        )
            .partial_cmp(&(
                other.trigger_price.into_inner() * ls_other,
                other.volume,
                other.limit_price.map(|p| p.into_inner() * ls_self),
            ))
    }
}

impl Ord for ConditionalOrder {
    fn cmp(&self, other: &Self) -> Ordering {
        let ls_self = self.trade_direction();
        let ls_other = other.trade_direction();

        (
            ordered_float::OrderedFloat(self.trigger_price.into_inner() * ls_self),
            self.volume,
            self.limit_price
                .map(|p| ordered_float::OrderedFloat(p.into_inner() * ls_other)),
        )
            .cmp(&(
                ordered_float::OrderedFloat(other.trigger_price.into_inner() * ls_other),
                other.volume,
                other
                    .limit_price
                    .map(|p| ordered_float::OrderedFloat(p.into_inner() * ls_self)),
            ))
    }
}

impl Hash for ConditionalOrder {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.id.hash(h)
    }
}

impl From<ConditionalOrder> for (UnconditionalOrder, OrderComplement) {
    fn from(stop_order: ConditionalOrder) -> Self {
        // Determine unconditional order information
        let buy = if let ConditionalType::StopLoss = stop_order.condition {
            !stop_order.buy
        } else {
            stop_order.buy
        };

        (
            UnconditionalOrder {
                limit_price: stop_order.limit_price,
                buy,
                id: stop_order.id,
                volume: stop_order.volume,
                created_at: stop_order.created_at,
                filled_at: None,
            },
            OrderComplement {
                trigger_price: stop_order.trigger_price,
                condition: stop_order.condition,
            },
        )
    }
}

impl From<(UnconditionalOrder, OrderComplement)> for ConditionalOrder {
    fn from(order_tuple: (UnconditionalOrder, OrderComplement)) -> Self {
        // Parse complementary information
        let buy = if let ConditionalType::StopLoss = order_tuple.1.condition {
            !order_tuple.0.buy
        } else {
            order_tuple.0.buy
        };

        ConditionalOrder {
            limit_price: order_tuple.0.limit_price,
            buy,
            id: order_tuple.0.id,
            volume: order_tuple.0.volume,
            created_at: order_tuple.0.created_at,
            filled_at: None,
            trigger_price: order_tuple.1.trigger_price,
            condition: order_tuple.1.condition,
        }
    }
}

impl UnconditionalOrder {
    pub fn new(id: u32, buy: bool, volume: u32, limit_price: Option<f32>) -> UnconditionalOrder {
        UnconditionalOrder {
            id,
            buy,
            limit_price: limit_price.map(ordered_float::OrderedFloat::from),
            volume,
            created_at: Local::now(),
            filled_at: None,
        }
    }

    /// Marks the order as filled.
    #[inline]
    pub fn fill(&mut self) {
        self.filled_at = Some(Local::now());
    }

    /// Splits off a part of the order into a new one.
    #[inline]
    pub fn split_at_volume(&mut self, vol_to_split: u32) -> UnconditionalOrder {
        self.volume = self.volume.saturating_sub(vol_to_split);
        UnconditionalOrder {
            volume: vol_to_split,
            ..*self
        }
    }

    /// Becomes 1 and -1 for a buy and sell order.
    #[inline]
    pub fn trade_direction(&self) -> f32 {
        (2 * (self.buy as i8) - 1) as f32
    }
}

impl PartialEq for UnconditionalOrder {
    fn eq(&self, other: &Self) -> bool {
        (self.id, self.volume) == (other.id, other.volume)
    }
}

impl PartialOrd for UnconditionalOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let ls_self = self.trade_direction();
        let ls_other = other.trade_direction();

        (
            self.limit_price.map(|p| p.into_inner() * ls_self),
            self.volume,
        )
            .partial_cmp(&(
                other.limit_price.map(|p| p.into_inner() * ls_other),
                other.volume,
            ))
    }
}

impl Ord for UnconditionalOrder {
    fn cmp(&self, other: &Self) -> Ordering {
        let ls_self = self.trade_direction();
        let ls_other = other.trade_direction();

        (
            self.limit_price
                .map(|p| ordered_float::OrderedFloat(p.into_inner() * ls_self)),
            self.volume,
        )
            .cmp(&(
                other
                    .limit_price
                    .map(|p| ordered_float::OrderedFloat(p.into_inner() * ls_other)),
                other.volume,
            ))
    }
}

impl Hash for UnconditionalOrder {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.id.hash(h)
    }
}
