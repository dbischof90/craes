@0xaae3995f79c25cb8;

struct OrderMsg {
    buy @0 :Bool;
    volume @1 :UInt32;
    limitprice :union {
        none @2 :Void;
        some @3 :Float32;
    }
    condition :union {
        unconditional @4 :Void;
        stoploss @5 :Float32;
        stopandreverse @6 :Float32;
    }
    assetname @7 :UInt16;
}


struct VolumeTradedAtPrice {
    volume @0 :UInt32;
    price @1 :Float32;
}


struct ResponseMsg {
    executedtrades @0 :List(VolumeTradedAtPrice);
}
