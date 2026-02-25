use clap::{Parser, ValueEnum};
mod relay;
mod client;

#[derive(Debug, Parser, Clone)]
struct Opts {
    // Si ce noeud est celui qui initie la connection, celui qui la recoit, voir le relai
    #[arg(long, value_enum)]
    mode: Mode,

    // Adresse du relai qui va permettre le hole punching
    #[arg(long, required_if_eq_any([("mode", "dial"), ("mode", "listen")]), help("Peers in dial/listen mode require '--relay-ip'"))]
    relay_ip: Option<String>,
    #[arg(long, required_if_eq_any([("mode", "dial"), ("mode", "listen")]), help("Peers in dial/listen mode require '--relay-port'"))]
    relay_port: Option<u16>,

    // L'adresse ip du noeud auquel l' (dial) veut se connecter
    #[arg(long, required_if_eq("mode", "dial"), help("Peers in dial mode require '--remote-peer-ip'"))]
    remote_peer_ip: Option<String>,
    #[arg(long, required_if_eq("mode", "dial"), help("Peers in dial mode require '--remote-peer-port'"))]
    remote_peer_port: Option<String>,
}

#[derive(Clone, Debug, PartialEq, ValueEnum)]
enum Mode {
    Dial,
    Listen,
    Relay,
}


#[tokio::main]
async fn main() {
	// Récupération du type de noeud (dial/listen/relay)
    let opts = Opts::parse();

    match opts.mode {
        Mode::Relay => relay::main_relay().await,
        Mode::Listen | Mode::Dial => client::main_client(opts.clone()).await,
    }
}
