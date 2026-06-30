use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

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
    windows: HashMap<u64, Window>,
}

enum WindowIdentifier<'a> {
    Str(&'a str),
    Numeric(u64),
}

impl EventsState {
    pub fn new(doc: KdlDocument) -> Self {
        Self {
            document: doc,
            windows: HashMap::new(),
        }
    }

    fn did_window_spawn(&self, id: u64) -> bool {
        !self.windows.contains_key(&id)
    }
    fn window(&self, id: u64) -> &Window {
        self.windows
            .get(&id)
            .expect("Window with the provided id should be mapped")
    }

    pub fn handle_events_for_node(&self, node: &str, handler: &str) {
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

    pub fn handle_events_of(&self, window: &Window, handler: &str) {
        let node = match () {
            _ if let Some(ref id) = window.app_id => id.clone(),
            _ if let Some(ref title) = window.title => title.clone(),
            _ if let Some(ref pid) = window.pid => pid.to_string(),
            _ => window.id.to_string(),
        };
        self.handle_events_for_node(&node, handler);
    }

    pub fn run(mut self) -> Result<()> {
        let mut socket = Socket::connect()?;
        let windows = socket.send(Request::Windows)?;
        if let Response::Windows(windows) = windows.map_err(|e| color_eyre::Report::msg(e))? {
            for window in windows {
                println!("{}", window.id);
                self.windows.insert(window.id, window);
            }
        }
        let Ok(Response::Handled) = socket.send(Request::EventStream)? else {
            return Ok(());
        };
        let mut reader = socket.read_events();
        while let Ok(event) = reader() {
            match event {
                Event::WindowClosed { id } => {
                    let Some(window) = self.windows.get(&id) else {
                        eprintln!("Window should be mapped");
                        continue;
                    };
                    self.handle_events_of(window, "on-close");
                    self.windows.remove(&id);
                }
                Event::WindowOpenedOrChanged { window } if self.did_window_spawn(window.id) => {
                    self.handle_events_of(&window, "on-spawn");
                    self.windows.insert(window.id, window);
                }
                Event::WindowOpenedOrChanged { window } if !self.did_window_spawn(window.id) => {
                    self.handle_events_of(&window, "on-change")
                }
                Event::WindowFocusChanged { id } if let Some(id) = id => {
                    self.handle_events_of(self.window(id), "on-focus")
                }
                Event::OverviewOpenedOrClosed { is_open } if is_open => {
                    self.handle_events_for_node("overview", "on-open")
                }
                Event::OverviewOpenedOrClosed { is_open } if !is_open => {
                    self.handle_events_for_node("overview", "on-close")
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
