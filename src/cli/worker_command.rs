use clap::{arg, Arg, ArgMatches, Command};
use log::debug;

pub(crate) fn create_worker_command() -> Command {
    let command = Command::new("worker")
        .about("Start a worker that will process documents.");
    command
}

