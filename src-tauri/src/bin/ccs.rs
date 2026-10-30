//! `ccs`: CC Switch command-line companion (PRODUCT-11). See docs/cli.md.

fn main() {
    std::process::exit(cc_switch_lib::cli::main());
}
