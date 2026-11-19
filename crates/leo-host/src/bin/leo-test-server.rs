use anyhow::{bail, Context, Result};

const FAKE_ANSWER: &str = "Yes. Argon2id, then verify at login.";

fn port_from(args: &[String]) -> Result<u16> {
    match args {
        [flag, port] if flag == "--port" => port.parse().context("--port needs a number"),
        _ => bail!("usage: leo-test-server --port <port>"),
    }
}

fn main() -> Result<()> {
    if leo_services::speech::is_worker_invocation() {
        std::process::exit(leo_services::speech::run_worker());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = port_from(&args)?;
    let chats = leo_web::chats::dir_for(&leo_core::store::Store::notes_dir()?);
    leo_web::margin::set_prefs(
        &chats,
        &leo_web::margin::Prefs {
            default_mode: "ask".into(),
            ..leo_web::margin::prefs(&chats)
        },
    );
    let mut powers = leo_host::powers();
    if let Ok(reply) = std::env::var("LEO_TEST_FAKE_AI") {
        let pause = std::env::var("LEO_TEST_FAKE_AI_MS")
            .ok()
            .and_then(|ms| ms.parse().ok())
            .map(std::time::Duration::from_millis)
            .unwrap_or_default();
        powers.toolless = Some(std::sync::Arc::new(move |_, _, _| {
            std::thread::sleep(pause);
            Ok(reply.clone())
        }));
        powers.toolless_chat = Some(std::sync::Arc::new(|_, _, _, _, _, _| {
            Ok(leo_web::Reply::from(FAKE_ANSWER.to_string()))
        }));
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let ended = runtime.block_on(async {
        let running = leo_web::start(
            leo_web::StartOptions {
                port,
                token: leo_web::saved_token()?,
            },
            powers,
        )
        .await?;
        tokio::signal::ctrl_c().await?;
        running.stop().await;
        Ok(())
    });
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    ended
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_comes_from_its_flag() {
        assert_eq!(
            port_from(&["--port".into(), "31831".into()]).unwrap(),
            31831
        );
        assert!(port_from(&[]).is_err());
        assert!(port_from(&["--port".into(), "x".into()]).is_err());
    }
}
