use std::{io::Write, process::Stdio};

use lazy_regex::regex;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tracing::{debug, error, info};
use webhook::client::{WebhookClient, WebhookResult};

async fn send_factorio_notification(
    endpoint: &str,
    is_join: bool,
    player_name: &str,
) -> WebhookResult<()> {
    let client = WebhookClient::new(endpoint);
    let content = if is_join {
        format!("{} が Factorio サーバーに参加しました", player_name)
    } else {
        format!("{} が Factorio サーバーから退出しました", player_name)
    };

    client
        .send(|message| message.username("FactorioWatch").content(&content))
        .await?;

    Ok(())
}

async fn watch_factorio(
    factorio_path: &str,
    factorio_args: Vec<String>,
    endpoint: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::new(factorio_path);
    cmd.args(&factorio_args);
    cmd.stdout(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn factorio process");

    let stdout = child
        .stdout
        .take()
        .expect("child did not have a handle to stdout");
    let mut reader = BufReader::new(stdout).lines();

    let endpoint = endpoint.to_owned();
    tokio::spawn(async move {
        let endpoint = endpoint.clone();
        let join_re = regex!(r"\[JOIN\] (.*) joined the game$");
        let leave_re = regex!(r"\[LEAVE\] (.*) left the game$");

        while let Some(line) = reader.next_line().await.unwrap() {
            std::io::stdout().write_all(line.as_bytes()).unwrap();
            std::io::stdout().write_all(b"\n").unwrap();

            if let Some(caps) = join_re.captures(&line) {
                let name = caps.get(1).unwrap().as_str();
                if let Err(e) = send_factorio_notification(&endpoint, true, name).await {
                    error!("send_factorio_notification failed: {:?}", e);
                } else {
                    info!(
                        "send_factorio_notification success: player={}, is_join=true",
                        name
                    );
                }
            } else if let Some(caps) = leave_re.captures(&line) {
                let name = caps.get(1).unwrap().as_str();
                if let Err(e) = send_factorio_notification(&endpoint, false, name).await {
                    error!("send_factorio_notification failed: {:?}", e);
                } else {
                    info!(
                        "send_factorio_notification success: player={}, is_join=false",
                        name
                    );
                }
            }
        }
    });

    let (tx, rx) = tokio::sync::oneshot::channel::<i32>();
    let mut tx_option = Some(tx);

    ctrlc::set_handler(move || {
        if let Some(tx) = tx_option.take() {
            tx.send(0).unwrap();
        }
    })
    .expect("Error setting Ctrl-C handler");

    tokio::select! {
        _ = rx => {
            info!("Received SIGINT. Stopping factorio...");
            // if let Some(pid) = child.id() {
            //     nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGINT).unwrap();
            // }
            let status = child.wait().await?;
            info!("Factorio exited with: {}", status);
        },
        status = child.wait() => {
            info!("Factorio exited with: {}", status?);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // get argument
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!(
            "Usage: {} <factorio_path> <endpoint> <factorio_args>",
            args[0]
        );
        return;
    }

    let factorio_path = &args[1];
    let endpoint = &args[2];
    let factorio_args = args[3..].to_vec();
    debug!("factorio_path: {}", factorio_path);
    debug!("endpoint: {}", endpoint);
    debug!("factorio_args: {:?}", factorio_args);

    if let Err(e) = watch_factorio(factorio_path, factorio_args, endpoint).await {
        error!("watch_factorio failed: {:?}", e);
    }
}
