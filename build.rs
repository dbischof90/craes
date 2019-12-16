use capnpc;

fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/protocol.capnp")
        .run()
        .expect("Compiling schema");
}
