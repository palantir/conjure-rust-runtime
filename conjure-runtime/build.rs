use std::env;

fn main() {
    let feature_ring = env::var("CARGO_FEATURE_RING").is_ok();
    let feature_aws_lc_rs = env::var("CARGO_FEATURE_AWS_LC_RS").is_ok();

    if feature_ring && feature_aws_lc_rs {
        panic!("Features `ring` and `aws-lc-rs` are mutually exclusive and cannot be enabled at the same time.");
    }
}
