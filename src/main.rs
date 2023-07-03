use clap::{Arg, Command};
use crossterm::event;
// use env_logger::{self, Builder};
use futures::StreamExt;
use log::{error, LevelFilter};
use std::{io, io::Write, sync::Arc};
use tokio::sync::mpsc;
use tui::{backend::CrosstermBackend, Terminal};

mod ui;

use ui::{KtxApp, KtxEvent, RendererMessage};

#[tokio::main]
async fn main() {
    // Builder::new()
    //     .format(|buf, record| {
    //         writeln!(
    //             buf,
    //             "{} [{}] - {}",
    //             chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
    //             record.level(),
    //             record.args()
    //         )
    //     })
    //     .filter(None, LevelFilter::Info)
    //     .init();
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
    let app_clone = app.clone();

    app.start().await;

    let renderer = tokio::spawn(async move {
        app_clone.start_renderer(renderer_rx).await;
    });

    let event_handler = tokio::spawn(async move {
        let mut reader = event::EventStream::new();
        loop {
            renderer_tx.send(RendererMessage::Render).await.unwrap();
            tokio::select! {
                terminal_event = reader.next() => {
                    let evt = terminal_event.expect("Failed to read event").unwrap();
                    app.handle_event(KtxEvent::TerminalEvent(evt)).await.unwrap();
                },
                app_event = event_bus_rx.recv() => {
                    let evt = app_event.expect("Failed to read event");
                    match evt {
                        KtxEvent::Exit => {
                            break;
                        },
                        _ => {
                            app.handle_event(evt).await.unwrap();
                        },
                    }
                },
            }
        }
        renderer_tx.send(RendererMessage::Stop).await.unwrap();
        app.shutdown().await;
    });
    let (_, _) = tokio::join!(renderer, event_handler);
}
