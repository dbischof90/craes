@0xaae3995f79c25cb8;

struct OrderMsg {
    buy @0 :Bool;
    volume @1 :Int32;
    limitprice :union {
        none @2 :Void;
        some @3 :Int32;
    }
    condition :union {
        unconditional @4 :Void;
        stoporder :group {
            stop @5 :Int32;
        }
    }
    assetname @6 :UInt16;
}


struct VolumeTradedAtPrice {
    volume @0 :Int32;
    price @1 :Int32;
}


struct ResponseMsg {
    executedtrades @0 :List(VolumeTradedAtPrice);
}
