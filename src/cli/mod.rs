mod installation;
mod open_app;
mod prompt;
mod uninstall;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "leo", version, about = "Open leo", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Remove leo and everything it made from this computer. Your notes stay")]
    Uninstall {
        #[arg(short, long, help = "Do not ask first")]
        yes: bool,
    },
}

pub fn parse() -> Cli {
    Cli::parse()
}

#[cfg(test)]
pub fn parse_from<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::parse_from(args)
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Commands::Uninstall { yes }) => uninstall::run(yes),
        None => open_app::open(),
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn bare_leo_opens_the_app() {
        assert!(parse_from(["leo"]).command.is_none());
    }

    #[test]
    fn only_uninstall_remains() {
        assert!(matches!(
            parse_from(["leo", "uninstall", "--yes"]).command,
            Some(Commands::Uninstall { yes: true })
        ));
        for args in [&["leo", "update"][..], &["leo", "update", "--force"]] {
            assert_eq!(
                Cli::try_parse_from(args).err().map(|e| e.kind()),
                Some(clap::error::ErrorKind::InvalidSubcommand)
            );
        }
        for gone in [
            "serve", "doctor", "new", "list", "backup", "trash", "record", "ask", "help", "update",
        ] {
            assert!(
                Cli::try_parse_from(["leo", gone]).is_err(),
                "{gone} still parses"
            );
        }
    }

    #[test]
    fn the_version_flag_still_works() {
        let err = Cli::try_parse_from(["leo", "--version"]).err().unwrap();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_a_mac_leo_opens_by_bundle_id() {
        let cmd = open_app::open_command().unwrap();
        assert_eq!(cmd.get_program(), "open");
        assert_eq!(
            cmd.get_args().collect::<Vec<_>>(),
            ["-b", "io.github.chewton2k.leo"]
        );
    }
}
