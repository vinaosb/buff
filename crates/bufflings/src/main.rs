//! Binary entry point for `bufflings`. Thin dispatch to lib.

use bufflings::{run, Cli};
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    run(cli)
}
