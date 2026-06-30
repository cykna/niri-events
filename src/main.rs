use std::{collections::HashSet, path::PathBuf};

use clap::Parser;
use color_eyre::eyre::Result;
use kdl::KdlDocument;
use niri_ipc::{Event, Request, Response, socket::Socket};

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    path: Option<String>,
}

struct EventsState {
    socket: Socket,
    document: KdlDocument,
    ids: HashSet<u64>,
}

impl EventsState {
    pub fn new(doc: KdlDocument) -> Result<Self> {
        Ok(Self {
            document: doc,
            socket: Socket::connect()?,
            ids: HashSet::new(),
        })
    }
    pub fn run(mut self) -> Result<()> {
        let Ok(Response::Handled) = self.socket.send(Request::EventStream)? else {
            return Ok(());
        };
        let mut reader = self.socket.read_events();
        while let Ok(event) = reader() {
            match event {
                Event::WindowClosed { id } => {
                    self.ids.remove(&id);
                }
                Event::WindowOpenedOrChanged { window }
                    if let Some(ref id) = window.app_id
                        && let Some(document) = self.document.get(id) =>
                {
                    if self.ids.contains(&window.id) {
                        continue;
                    }
                    self.ids.insert(window.id);
                    let Some(children) = document.children() else {
                        continue;
                    };
                    let Some(on_spawn_children) = children
                        .get("on-spawn")
                        .map(|node| node.children())
                        .flatten()
                    else {
                        continue;
                    };
                    let commands: Vec<_> = on_spawn_children
                        .nodes()
                        .iter()
                        .map(|child| {
                            let mut out = tokio::process::Command::new("bash");
                            out.arg("-c").arg(child.name().value());
                            out
                        })
                        .collect();

                    tokio::spawn(async {
                        for mut cmd in commands {
                            let mut child = cmd.spawn().expect("Error on initialize bash process");
                            if let Err(e) = child.wait().await {
                                println!("Error on waiting async command '{cmd:?}': {e}");
                            }
                        }
                    });
                    println!("{on_spawn_children:?}");
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
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

    let document: KdlDocument = content.parse()?;
    let state = EventsState::new(document)?;
    state.run()
}
