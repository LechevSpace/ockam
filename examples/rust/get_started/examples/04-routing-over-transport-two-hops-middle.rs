// This node creates a tcp connection to a node at 127.0.0.1:4000
// Starts a tcp listener at 127.0.0.1:3000
// It then runs forever waiting to route messages.

use ockam::{Context, Result, TcpTransport};

#[ockam::node]
async fn main(ctx: Context) -> Result<()> {
    // Initialize the TCP Transport.
    let tcp = TcpTransport::create(&ctx).await?;

    // Create a TCP listener and wait for incoming connections.
    // Use port 3000, unless otherwise specified by command line argument.
    let port = std::env::args().nth(1).unwrap_or_else(|| "3000".to_string());
    tcp.listen(format!("127.0.0.1:{port}")).await?;

    // Don't call ctx.stop() here so this node runs forever.
    Ok(())
}
