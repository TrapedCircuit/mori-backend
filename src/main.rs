use std::net::SocketAddr;
use std::str::FromStr;

use aleo_rust::{Network, PrivateKey, Testnet3};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Json;
use backend::Execution;
use backend::{cores::GameNode, Mori};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
#[derive(Debug, Parser)]
#[clap(name = "mori-backend")]
pub struct Cli {
    #[clap(long)]
    pub ai_dest: String,

    #[clap(long)]
    pub ai_token: String,

    #[clap(long)]
    pub aleo_rpc: Option<String>,

    #[clap(long)]
    pub pk: String,

    #[clap(long)]
    pub port: u16,

    #[clap(long, default_value = "0")]
    pub from_height: u32,

    #[clap(long, default_value = "mori.aleo")]
    pub program_name: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let cli = Cli::parse();
    let Cli {
        ai_dest,
        ai_token,
        aleo_rpc,
        pk,
        port,
        from_height,
        program_name,
    } = cli;

    // Init Mori Aleo
    let pk = PrivateKey::<Testnet3>::from_str(&pk).expect("Invalid private key");
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let mori = Mori::new(aleo_rpc, pk, tx, program_name, ai_dest, ai_token)
        .expect("Failed to initialize Mori");
    // set from height
    mori.set_cur_height(from_height)
        .expect("Failed to set from height");
    let mori = mori.initial(rx);

    // Init Mori Rest
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([axum::http::header::CONTENT_TYPE]);

    let router = axum::Router::new()
        .route("/node/list", get(list_nodes))
        .route("/open_game", post(open_game))
        .with_state(mori)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");
    axum::serve(listener, router.into_make_service())
        .await
        .expect("Failed to serve");
}

async fn list_nodes<N: Network>(
    State(mori): State<Mori<N>>,
) -> anyhow::Result<Json<NodesResponse>, (StatusCode, String)> {
    let nodes = match mori.get_all_nodes() {
        Ok(nodes) => nodes,
        Err(e) => {
            tracing::error!("Failed to get all nodes: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get all nodes: {}", e),
            ));
        }
    };
    let nodes = NodesResponse { nodes };

    Ok(Json(nodes))
}

async fn open_game<N: Network>(
    State(mori): State<Mori<N>>,
) -> anyhow::Result<String, (StatusCode, String)> {
    let exec = Execution::OpenGame;
    mori.tx.send(exec).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to send execution: {}", e),
        )
    })?;

    Ok("alreay add in execution pipeline".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodesResponse {
    nodes: Vec<(u128, GameNode)>,
}
