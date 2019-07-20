use crate::order::{Order, OrderCondition};
use boolinator::Boolinator;
use stacker;
use std::cmp::Ordering;
use std::collections::binary_heap::PeekMut;
use std::collections::hash_map::Entry;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::mem;

#[derive(Debug)]
pub struct Orderbook {
    buy_orders: BinaryHeap<Order>,
    sell_orders: BinaryHeap<Order>,
    symbol: String,
}

impl Orderbook {
    pub fn new(symbol: String) -> Orderbook {
        Orderbook {
            buy_orders: BinaryHeap::new(),
            sell_orders: BinaryHeap::new(),
            symbol,
        }
    }

    pub fn len(&self) -> (usize, usize) {
        (self.buy_orders.len(), self.sell_orders.len())
    }

    pub fn resolve_order(&mut self, order_to_resolve: Order) -> HashMap<Order, Vec<Order>> {
        let buy_orders = mem::replace(&mut self.buy_orders, BinaryHeap::new());
        let sell_orders = mem::replace(&mut self.sell_orders, BinaryHeap::new());
        if order_to_resolve.buy {
            let (sell_orders, buy_orders, trade_set) =
                Self::trade_orders(sell_orders, buy_orders, order_to_resolve);
            self.sell_orders = sell_orders;
            self.buy_orders = buy_orders;
            trade_set
        } else {
            let (buy_orders, sell_orders, trade_set) =
                Self::trade_orders(buy_orders, sell_orders, order_to_resolve);
            self.sell_orders = sell_orders;
            self.buy_orders = buy_orders;
            trade_set
        }
    }

    pub fn cancel_order(&mut self, order_to_cancel: Order) -> Result<(), &'static str> {
        let order_heap = if order_to_cancel.buy {
            &mut self.buy_orders
        } else {
            &mut self.sell_orders
        };

        let tested_trades = &mut BinaryHeap::with_capacity(order_heap.len());
        while let Some(head_order) = order_heap.pop() {
            if head_order == order_to_cancel {
                order_heap.append(tested_trades);
                return Ok(());
            } else {
                tested_trades.push(head_order);
            }
        }
        Err("Order was not in orderbook.")
    }

    fn trade_orders(
        mut book_to_resolve: BinaryHeap<Order>,
        mut order_heap: BinaryHeap<Order>,
        mut trade_to_resolve: Order,
    ) -> (
        BinaryHeap<Order>,
        BinaryHeap<Order>,
        HashMap<Order, Vec<Order>>,
    ) {
        let mut matched_orders: Vec<Order> = Vec::new();
        let (subsequent_trade, subsequential_conditional, resolvable) = {
            let mut book_head = book_to_resolve.peek_mut();
            let (quantitative_relation, order_activable, book_not_empty) = match book_head {
                None => (None, false, false),
                Some(ref mut head_order) => match head_order.condition {
                    OrderCondition::Unconditional => (
                        trade_to_resolve
                            .is_active_with(&(*head_order))
                            .as_some(trade_to_resolve.volume.cmp(&(*head_order).volume)),
                        trade_to_resolve.is_active_with(&(*head_order)),
                        true,
                    ),
                    OrderCondition::Stop { .. } => {
                        head_order.condition = OrderCondition::Unconditional;
                        head_order.buy != head_order.buy;
                        (None, trade_to_resolve.is_active_with(&(*head_order)), true)
                    }
                },
            };

            let (subsequent_trade, subsequential_conditional) =
                if let Some(comparison) = quantitative_relation {
                    let mut head_order_peek = book_head.unwrap();
                    let subsequent_trade = match comparison {
                        Ordering::Equal => {
                            let mut head_order = PeekMut::pop(head_order_peek);
                            head_order.fill();
                            matched_orders.push(head_order);
                            None
                        }
                        Ordering::Less => {
                            let mut head_order =
                                head_order_peek.split_at_volume(trade_to_resolve.volume);
                            head_order.fill();
                            matched_orders.push(head_order);
                            None
                        }
                        Ordering::Greater => {
                            trade_to_resolve.volume -= head_order_peek.volume;
                            let mut head_order = PeekMut::pop(head_order_peek);
                            head_order.fill();
                            matched_orders.push(head_order);
                            Some(trade_to_resolve)
                        }
                    };
                    (subsequent_trade, None)
                } else {
                    (
                        Some(trade_to_resolve),
                        book_head
                            .filter(|_| order_activable)
                            .map(|x| PeekMut::pop(x)),
                    )
                };
            (
                subsequent_trade,
                subsequential_conditional,
                order_activable & book_not_empty,
            )
        };

        let mut resolved_trades = HashMap::<Order, Vec<Order>>::default();
        if resolvable {
            stacker::maybe_grow(32 * 1024, 2 * 1024 * 1024, || {
                let (order_heap, book_to_resolve, resolved_conditional_trades) =
                    match subsequential_conditional {
                        Some(order) => Self::trade_orders(order_heap, book_to_resolve, order),
                        None => (
                            book_to_resolve,
                            order_heap,
                            HashMap::<Order, Vec<Order>>::default(),
                        ),
                    };

                let (book_to_resolve, order_heap, mut resolved_trades) = match subsequent_trade {
                    Some(order) => Self::trade_orders(book_to_resolve, order_heap, order),
                    None => (
                        book_to_resolve,
                        order_heap,
                        HashMap::<Order, Vec<Order>>::default(),
                    ),
                };

                match resolved_trades.entry(trade_to_resolve) {
                    Entry::Occupied(entry) => {
                        entry.into_mut().extend(matched_orders);
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(matched_orders);
                    }
                };

                resolved_trades.extend(resolved_conditional_trades);
                (book_to_resolve, order_heap, resolved_trades)
            })
        } else {
            match trade_to_resolve.condition {
                OrderCondition::Unconditional => {
                    match trade_to_resolve.limit_price {
                        Some(_) => {
                            order_heap.push(trade_to_resolve);
                        }
                        None => {
                            resolved_trades.insert(trade_to_resolve, Vec::<Order>::new());
                        }
                    };
                }
                OrderCondition::Stop { .. } => order_heap.push(trade_to_resolve),
            };
            (book_to_resolve, order_heap, resolved_trades)
        }
    }
}
