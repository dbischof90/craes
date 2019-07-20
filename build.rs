use capnpc;

fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/ordermsg.capnp")
        .run()
        .expect("Compiling schema");
}
