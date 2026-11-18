fn main() {
    tonic_build::compile_protos("../proto/relay.proto").unwrap();
}
