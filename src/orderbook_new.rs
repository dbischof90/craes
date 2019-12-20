#![allow(dead_code)]

use crate::order_new::{ConditionalOrder, Order, UnconditionalOrder};
use stacker;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::HashMap;

/// A limit order book. Contains both limit and stop order books.
#[derive(Debug)]
pub struct Orderbook {
    pub limit_buy_orders: BinaryHeap<UnconditionalOrder>,
    pub limit_sell_orders: BinaryHeap<UnconditionalOrder>,
    pub stop_buy_orders: BinaryHeap<ConditionalOrder>,
    pub stop_sell_orders: BinaryHeap<ConditionalOrder>,
    pub symbol: String,
}

impl Orderbook {
    /// Constructs a new book.
    pub fn new(symbol: String) -> Orderbook {
        Orderbook {
            limit_buy_orders: BinaryHeap::new(),
            limit_sell_orders: BinaryHeap::new(),
            stop_buy_orders: BinaryHeap::new(),
            stop_sell_orders: BinaryHeap::new(),
            symbol,
        }
    }

    /// Main method to resolve an incoming order.
    pub fn resolve_order(
        &mut self,
        order_to_resolve: Order,
    ) -> HashMap<Order, Vec<UnconditionalOrder>> {
        let mut trades: HashMap<Order, Vec<UnconditionalOrder>> = HashMap::new();

        // In case the incoming order is a limit order, a more general resolution function
        // is called. Stop orders are purely passive and need to be triggered, hence they
        // are placed into the books immediately.
        match order_to_resolve {
            Order::LimitMarket(order) => trade_orders(
                order,
                &mut trades,
                &mut self.limit_buy_orders,
                &mut self.limit_sell_orders,
                &mut self.stop_buy_orders,
                &mut self.stop_sell_orders,
            ),
            Order::StopLimit(order) => {
                if order.buy {
                    self.stop_buy_orders.push(order)
                } else {
                    self.stop_sell_orders.push(order)
                }
            }
        }
        trades
    }
}

/// Recursive trade execution. Mutates the existing order books
/// and triggers conditional trades during trade resolution.
fn trade_orders(
    mut order_to_resolve: UnconditionalOrder,
    trades: &mut HashMap<Order, Vec<UnconditionalOrder>>,
    active_limit_book: &mut BinaryHeap<UnconditionalOrder>,
    backlog_limit_book: &mut BinaryHeap<UnconditionalOrder>,
    active_stop_book: &mut BinaryHeap<ConditionalOrder>,
    backlog_stop_book: &mut BinaryHeap<ConditionalOrder>,
) {
    // Initial calculation and allocation
    let ls = order_to_resolve.trade_direction();
    let limit_price_priority_f64 = order_to_resolve.limit_price_priority();
    let mut resolved_orders = Vec::new();
    loop {
        // Check whether a stop order is triggered by the execution.
        // Also triggers all recursive stop-order dependencies which are
        // resolved after this block and we can assume to have a standard
        // limit order resolution as the last step. 
        if let Some(existing_stop_order) = active_stop_book.peek() {
            if limit_price_priority_f64 * ls > existing_stop_order.stop_limit.into_inner() {
                let stop_order_to_execute = active_stop_book.pop().unwrap();
                // Extends the stack to the heap in case an overflow is apparent.
                stacker::maybe_grow(32 * 1024, 2 * 1024 * 1024, || {
                    trade_orders(
                        UnconditionalOrder::from(stop_order_to_execute),
                        trades,
                        backlog_limit_book,
                        active_limit_book,
                        backlog_stop_book,
                        active_stop_book,
                    )
                })
            }
        }

        // The next check considers the actual limit order book.
        // An order can execute a trade if it is eligible on the limit price level. If not,
        // it will be placed onto the limit order book for later execution.
        if let Some(next_limit_order) = active_limit_book.peek() {
            if limit_price_priority_f64 * ls > next_limit_order.limit_price_priority() * ls {
                match order_to_resolve.volume.cmp(&next_limit_order.volume) {
                    Ordering::Equal => {
                        // The order is written on the same volume and fills the remaining
                        // order size perfectly.
                        let mut executed_trade = active_limit_book.pop().unwrap();
                        executed_trade.fill();
                        resolved_orders.push(executed_trade);
                        break;
                    }
                    Ordering::Greater => {
                        // The order exceeds the volume available on the best buy/sell order.
                        // The front of the book is removed and the order is partially traded.
                        let mut executed_trade = active_limit_book.pop().unwrap();
                        executed_trade.fill();
                        order_to_resolve.volume -= executed_trade.volume;
                        resolved_orders.push(executed_trade);
                    }
                    Ordering::Less => {
                        // The front of the orderbook exceeds the order to resolve in size
                        // and is traded partially.
                        let mut mutable_front_book = active_limit_book.peek_mut().unwrap();
                        let mut executed_trade =
                            mutable_front_book.split_at_volume(order_to_resolve.volume);
                        executed_trade.fill();
                        resolved_orders.push(executed_trade);
                        break;
                    }
                }
            } else {
                backlog_limit_book.push(order_to_resolve);
                break;
            }
        } else {
            // If there is no limit order left to trade against, the order is put into the order
            // book if it is not a market order.
            if let Some(_) = order_to_resolve.limit_price {
                backlog_limit_book.push(order_to_resolve);
                break;
            }
        }
    }

    // After all operations on the order books are completed, the resulting
    // trades are saved.
    trades.insert(Order::LimitMarket(order_to_resolve), resolved_orders);
}
