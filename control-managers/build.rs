
fn main() {
    if std::env::var_os("CARGO_FEATURE_GPU_NODE").is_some() {
        cc::Build::new()
            .cuda(true)
            .file("src/servernode-daemon/gpu_cores_getter.c")
            .compile("gpu_cores_getter");
    }
}
