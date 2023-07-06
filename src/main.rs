use clap::{Arg, Command};
use crossterm::event;
use futures::StreamExt;
use std::{io, sync::Arc};
use tokio::sync::mpsc;
use tui::{backend::CrosstermBackend, Terminal};

mod ui;

use ui::{KtxApp, KtxEvent, RendererMessage};

#[tokio::main]
async fn main() {
    let matches = Command::new("ktx")
        .version("0.1.0")
        .author("Maksim Leanovich <lm.bsod@gmail.com>")
        .about("Kubernetes config management tool")
        .arg(
            Arg::new("kubeconfig")
                .short('c')
                .long("kubeconfig")
                .value_name("FILE")
                .help("Sets a custom kubeconfig file"),
        )
        .get_matches();

    let default_config = shellexpand::tilde("~/.kube/config").into_owned();
    let config_path = matches
        .get_one::<String>("kubeconfig")
        .unwrap_or(&default_config)
        .clone();

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).expect("Failed to create terminal");
    let (renderer_tx, renderer_rx) = mpsc::channel(1024);
    let (event_bus_tx, mut event_bus_rx) = mpsc::channel(1024);
    let app = Arc::new(KtxApp::new(config_path.clone(), terminal, event_bus_tx));

    app.start().await;

    let renderer = tokio::spawn({
        let app = app.clone();
        async move {
            app.start_renderer(renderer_rx).await;
        }
    });

    let event_handler = tokio::spawn({
        let app = app.clone();
        async move {
            let mut reader = event::EventStream::new();
            loop {
                renderer_tx.send(RendererMessage::Render).await.unwrap();
                tokio::select! {
                    terminal_event = reader.next() => {
                        let evt = terminal_event.expect("Failed to read event").unwrap();
                        app.handle_event(KtxEvent::TerminalEvent(evt)).await;
                    },
                    app_event = event_bus_rx.recv() => {
                        let evt = app_event.expect("Failed to read event");
                        match evt {
                            KtxEvent::Exit => {
                                break;
                            },
                            _ => {
                                app.handle_event(evt).await;
                            },
                        }
                    },
                }
            }
            renderer_tx.send(RendererMessage::Stop).await.unwrap();
        }
    });
    let (_, _) = tokio::join!(renderer, event_handler);
    app.shutdown().await;
}
