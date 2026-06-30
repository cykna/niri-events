use std::path::PathBuf;

use clap::Parser;
use color_eyre::eyre::Result;
use kdl::KdlDocument;
use niri_ipc::{Request, Response, socket::Socket};

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    path: Option<String>,
}

fn main() -> Result<()> {
    let config = Cli::parse();
    let path = config.path.map(|v| PathBuf::from(v)).unwrap_or(
        std::env::home_dir()
            .expect("Should contain a home dir")
            .join(".config")
            .join("niri")
            .join("events.kdl"),
    );
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let document: KdlDocument = content.parse().expect("Error on parsing kdl");

    let mut socket = Socket::connect()?;
    if let Ok(Response::Handled) = socket.send(Request::EventStream)? {
        let mut reader = socket.read_events();
        while let Ok(event) = reader() {
            println!("{event:?}\nAki o KDL: {document:?}");
        }
    }
    Ok(())
}
