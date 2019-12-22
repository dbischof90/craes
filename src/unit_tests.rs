use crate::{order, orderbook};
use std::collections::HashMap;

#[test]
fn simple_buy_transaction() {
    let limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, false, 10, Some(10.0)));
    let market_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, true, 10, None));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(limit_sell);
    let trade = order_book.resolve_order(market_buy);

    // Build predicted result
    let mut trade_predicted = HashMap::new();
    trade_predicted.insert(
        market_buy,
        vec![order::UnconditionalOrder::new(1, false, 10, Some(10.0))],
    );

    assert_eq!(trade, trade_predicted)
}

#[test]
fn simple_sell_transaction() {
    let limit_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, true, 10, Some(10.0)));
    let market_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, false, 10, None));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(limit_buy);
    let trade = order_book.resolve_order(market_sell);

    // Build predicted result
    let mut trade_predicted = HashMap::new();
    trade_predicted.insert(
        market_sell,
        vec![order::UnconditionalOrder::new(1, true, 10, Some(10.0))],
    );

    assert_eq!(trade, trade_predicted)
}

#[test]
fn buy_with_limit_order() {
    let limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, false, 10, Some(10.0)));
    let limit_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, true, 10, Some(11.0)));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(limit_sell);
    let trade = order_book.resolve_order(limit_buy);

    // Build predicted result
    let mut trade_predicted = HashMap::new();
    trade_predicted.insert(
        limit_buy,
        vec![order::UnconditionalOrder::new(1, false, 10, Some(10.0))],
    );

    assert_eq!(trade, trade_predicted)
}

#[test]
fn sell_with_limit_order() {
    let limit_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, true, 10, Some(10.0)));
    let limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, false, 10, Some(9.0)));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(limit_buy);
    let trade = order_book.resolve_order(limit_sell);

    // Build predicted result
    let mut trade_predicted = HashMap::new();
    trade_predicted.insert(
        limit_sell,
        vec![order::UnconditionalOrder::new(1, true, 10, Some(10.0))],
    );

    assert_eq!(trade, trade_predicted)
}

#[test]
fn no_trades_at_market() {
    let limit_buy_low =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, true, 10, Some(9.0)));
    let limit_sell_high =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, false, 10, Some(12.0)));
    let limit_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, true, 10, Some(10.0)));
    let limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, false, 10, Some(11.0)));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(limit_buy);
    let _ = order_book.resolve_order(limit_buy_low);
    let _ = order_book.resolve_order(limit_sell_high);
    let trade = order_book.resolve_order(limit_sell);

    // Build predicted result
    let mut trade_predicted: HashMap<order::Order, Vec<order::UnconditionalOrder>> =
        HashMap::new();
    trade_predicted.insert(limit_sell, Vec::new());

    assert_eq!(trade, trade_predicted)
}

#[test]
fn partial_buy_transaction() {
    let limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, false, 10, Some(10.0)));
    let market_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, true, 5, None));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(limit_sell);
    let trade = order_book.resolve_order(market_buy);

    // Build predicted result
    let mut trade_predicted = HashMap::new();
    let limit_sell_inner = order::UnconditionalOrder::new(1, false, 5, Some(10.0));
    trade_predicted.insert(market_buy, vec![limit_sell_inner]);

    assert_eq!(trade, trade_predicted)
}

#[test]
fn two_step_buy_transaction() {
    let large_limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, false, 20, Some(10.0)));
    let limit_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, true, 10, Some(11.0)));
    let market_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(3, true, 10, None));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(large_limit_sell);
    let mut trade = order_book.resolve_order(limit_buy);
    let second_trade = order_book.resolve_order(market_buy);
    trade.extend(second_trade);

    // Build predicted result
    let mut trade_predicted = HashMap::new();
    let limit_sell_inner = order::UnconditionalOrder::new(1, false, 10, Some(10.0));
    trade_predicted.insert(market_buy, vec![limit_sell_inner]);
    trade_predicted.insert(limit_buy, vec![limit_sell_inner]);

    assert_eq!(trade, trade_predicted)
}

#[test]
fn simple_stop_buy_transaction() {
    let large_limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(1, false, 20, Some(12.0)));
    let small_limit_sell =
        order::Order::LimitMarket(order::UnconditionalOrder::new(2, false, 3, Some(11.0)));
    let large_limit_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(3, true, 20, Some(10.0)));
    let stop_market_sell = order::Order::StopLimit(order::ConditionalOrder::new(
        4,
        true,
        10,
        None,
        order::ConditionalType::StopAndReverse,
        11.5,
    ));

    let market_buy =
        order::Order::LimitMarket(order::UnconditionalOrder::new(5, true, 10, None));
    let mut order_book = orderbook::Orderbook::new("tester".to_string());

    // Trade orders
    let _ = order_book.resolve_order(large_limit_sell);
    let _ = order_book.resolve_order(small_limit_sell);
    let _ = order_book.resolve_order(large_limit_buy);
    let _ = order_book.resolve_order(stop_market_sell);
    let trade = order_book.resolve_order(market_buy);

    // Build predicted result
    let mut trade_predicted = HashMap::new();
    let trades_market_buy = vec![
        order::UnconditionalOrder::new(2, false, 3, Some(11.0)),
        order::UnconditionalOrder::new(1, false, 7, Some(12.0)),
    ];
    let trades_stop_sell = vec![order::UnconditionalOrder::new(3, true, 10, Some(10.0))];
    trade_predicted.insert(market_buy, trades_market_buy);
    trade_predicted.insert(stop_market_sell, trades_stop_sell);

    assert_eq!(trade, trade_predicted)
}
