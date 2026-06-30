use std::{collections::HashSet, path::PathBuf};

use clap::Parser;
use color_eyre::eyre::Result;
use kdl::KdlDocument;
use niri_ipc::{Event, Request, Response, Window, socket::Socket};

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    path: Option<String>,
}

struct EventsState {
    document: KdlDocument,
    ids: HashSet<u64>,
}

impl EventsState {
    pub fn new(doc: KdlDocument) -> Self {
        Self {
            document: doc,

            ids: HashSet::new(),
        }
    }

    fn did_window_spawn(&self, id: u64) -> bool {
        !self.ids.contains(&id)
    }

    pub fn handle_events_of(&self, window: Window, handler: &str) {
        let node = match () {
            _ if let Some(id) = window.app_id => id,
            _ if let Some(title) = window.title => title,
            _ if let Some(pid) = window.pid => pid.to_string(),
            _ => window.id.to_string(),
        };
        if let Some(doc) = self.document.get(&node)
            && let Some(children) = doc.children()
            && let Some(children) = children.get(handler).map(|node| node.children()).flatten()
        {
            let commands: Vec<_> = children
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
        };
    }

    pub fn run(mut self) -> Result<()> {
        let mut socket = Socket::connect()?;
        let Ok(Response::Handled) = socket.send(Request::EventStream)? else {
            return Ok(());
        };
        let mut reader = socket.read_events();
        while let Ok(event) = reader() {
            match event {
                Event::WindowClosed { id } => {
                    self.ids.remove(&id);
                }
                Event::WindowOpenedOrChanged { window } if self.did_window_spawn(window.id) => {
                    self.ids.insert(window.id);
                    self.handle_events_of(window, "on-spawn");
                }
                Event::WindowOpenedOrChanged { window } if !self.did_window_spawn(window.id) => {
                    self.handle_events_of(window, "on-change")
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
    let state = EventsState::new(document);
    state.run()
}
