use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, ExecuteCommand};

#[derive(Parser)]
pub struct MarkdownHelp;

impl ExecuteCommand for MarkdownHelp {
    async fn execute(self) -> Result<()> {
        clap_markdown::print_help_markdown::<Cli>();
        Ok(())
    }
}
