//! TrámitesUY ingestion worker CLI (IN-1, D-5): thin clap wiring over the
//! `ingestion` pipeline, the sqlx repository, and the taxonomy loader. All
//! pipeline logic lives in `crates/ingestion` and `crates/db`; this binary
//! only composes fetcher + repository + commands.

use clap::{Parser, Subcommand};

use ingest::commands;

#[derive(Parser)]
#[command(
    name = "ingest",
    about = "TrámitesUY ingestion and maintenance worker",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Resolve the AGESIC dataset via package_show, download the CSV
    /// resource, and run the diff/persist pipeline against the database.
    Ingest,
    /// Service mode (compose `ingest` service, task 88): run one ingestion
    /// pass, then sleep until 03:00 UTC and repeat daily, forever.
    Daemon,
    /// Load the YAML taxonomy (data/) into the database tables.
    SeedTaxonomy {
        /// Data directory holding events/, categories/, synonyms/.
        #[arg(long, default_value = "data")]
        data_dir: String,
        /// External-id snapshot the orphan check runs against (D-2).
        #[arg(long, default_value = "data/external_ids.snapshot.txt")]
        snapshot: String,
        /// Postgres URL (defaults to the compose dev database).
        #[arg(long)]
        database_url: Option<String>,
    },
    /// Write every ingested external_id to the committed snapshot file
    /// (D-2), sorted, LF line endings, trailing newline, byte-stable.
    ExportIds {
        /// Snapshot output path.
        #[arg(long, default_value = "data/external_ids.snapshot.txt")]
        output: String,
        /// Postgres URL (defaults to the compose dev database).
        #[arg(long)]
        database_url: Option<String>,
    },
    /// Build → validate → persist → promote the catalog generation
    /// (S6 task 18): the mandatory publication flow with its run record.
    Publish {
        /// Data directory holding events/, categories/, synonyms/.
        #[arg(long, default_value = "data")]
        data_dir: String,
        /// Postgres URL (defaults to the compose dev database).
        #[arg(long)]
        database_url: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Ingest => commands::ingest::run(),
        Command::Daemon => commands::daemon::run(),
        Command::SeedTaxonomy {
            data_dir,
            snapshot,
            database_url,
        } => commands::seed_taxonomy::run(&data_dir, &snapshot, database_url.as_deref()),
        Command::ExportIds {
            output,
            database_url,
        } => commands::export_ids::run(&output, database_url.as_deref()),
        Command::Publish {
            data_dir,
            database_url,
        } => commands::publish::run(&data_dir, database_url.as_deref()),
    }
}
