use anyhow::{Result, ensure};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() <= 1,
        "usage: openwam-preview-ui [port; default 8787]"
    );
    let port = args.first().map(|v| v.parse()).transpose()?.unwrap_or(8787);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let address = listener.local_addr()?;
    let state = reversal_sketch_intake::ui::App::new(address.port())?;
    println!(
        "Experimental OpenWAM preview UI: http://{address}. Primary interface: openwam request. Ctrl-C to stop."
    );
    axum::serve(listener, reversal_sketch_intake::ui::router(state))
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("registering Ctrl-C shutdown signal");
        })
        .await?;
    Ok(())
}
