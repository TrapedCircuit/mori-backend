use anyhow::anyhow;
use cores::{GameNode, MovRequest, RestResponse, Vote};
use once_cell::sync::OnceCell;
use snarkvm_ledger::{Input, Transition};
use std::str::FromStr;
use tokio::sync::mpsc::{Receiver, Sender};

use aleo_rust::{
    AleoAPIClient, Network, Plaintext, PrivateKey, ProgramID, ProgramManager, ViewKey
};
use db::{DBMap, RocksDB};
use filter::TransitionFilter;

use crate::{cores::GameState, utils::handle_u128_plaintext};

pub mod cores;
pub mod db;
pub mod filter;
pub mod utils;

pub const ALEO_NETWORK: &str = "testnet3";
static ALEO_CONTRACT: OnceCell<String> = OnceCell::new();
pub const FEE_NUM: u64 = 40000; // 0.04 aleo

#[derive(Clone)]
pub struct Mori<N: Network> {
    pm: ProgramManager<N>,
    pub aleo_client: AleoAPIClient<N>,
    filter: TransitionFilter<N>,
    pub tx: Sender<Execution>,

    ai_dest: String,
    ai_token: String,

    vk: ViewKey<N>,
    network_key: String, // <dest>-<pk>

    network_height: DBMap<String, u32>,
    mori_nodes: DBMap<u128, GameNode>, // <node_id, node>
}

impl<N: Network> Mori<N> {
    pub fn new(
        aleo_rpc: Option<String>,
        pk: PrivateKey<N>,
        tx: Sender<Execution>,
        program_name: String,
        ai_dest: String,
        ai_token: String,
    ) -> anyhow::Result<Self> {
        let aleo_client = match aleo_rpc {
            Some(aleo_rpc) => AleoAPIClient::new(&aleo_rpc, ALEO_NETWORK)?,
            None => AleoAPIClient::testnet3(),
        };
        let network_key = format!("{:?}-{}", aleo_client.network_id(), pk);
        tracing::info!("your private key is: {pk}, network key is {network_key}");

        tracing::info!("program name is {program_name}");
        ALEO_CONTRACT.set(program_name).map_err(|e| anyhow!(e))?;

        let vk = ViewKey::try_from(&pk)?;
        let pm = ProgramManager::new(Some(pk), None, Some(aleo_client.clone()), None, true)?;
        let filter =
            TransitionFilter::new().add_program(ProgramID::from_str(ALEO_CONTRACT.get().unwrap())?);

        let mori_nodes = RocksDB::open_map("mori_nodes")?;
        let network_height = RocksDB::open_map("network")?;

        let ai_token = format!(" Bearar {}", ai_token);

        Ok(Self {
            pm,
            aleo_client,
            filter,

            ai_dest,
            ai_token,

            tx,
            vk,
            mori_nodes,
            network_height,
            network_key,
        })
    }

    pub fn sync(&self) -> anyhow::Result<()> {
        let cur = self.network_height.get(&self.network_key)?.unwrap_or(0);
        let latest = self.aleo_client.latest_height()?;
        tracing::debug!("Requesting aleo blocks from {} to {}", cur, latest);
        const BATCH_SIZE: usize = 45;

        let ts_handler = move |transitions: Vec<Transition<N>>| {
            for t in transitions {
                match t.function_name().to_string().as_str() {
                    "vote" => self.handle_vote(t)?,
                    "move_to_next" => self.handle_move(t)?,
                    "open_game" => self.handle_open(t)?,
                    _ => {}
                }
            }
            Ok::<_, anyhow::Error>(())
        };

        for start in (cur..latest).step_by(BATCH_SIZE) {
            let end = (start + BATCH_SIZE as u32).min(latest);
            tracing::warn!("Fetched aleo blocks from {} to {}", start, end);
            let transitions = self
                .aleo_client
                .get_blocks(start, end)?
                .into_iter()
                .flat_map(|b| self.filter.filter_block(b))
                .collect::<Vec<Transition<N>>>();
            if let Err(e) = ts_handler(transitions) {
                tracing::error!("handle transitions error: {:?}", e);
            }
        }

        self.network_height.insert(&self.network_key, &latest)?;
        tracing::info!("Synced aleo blocks from {} to {}", cur, latest);
        Ok(())
    }

    pub fn execute_program(self, mut rx: Receiver<Execution>) -> anyhow::Result<()> {
        let handler = move |exec| {
            tracing::warn!("received execution: {:?}", exec);
            let (function, inputs) = match exec {
                Execution::MoveToNext(mov) => {
                    let game_state = GameState::from_vec_i8(&mov.state);
                    let parent_id = mov.parent_id.ok_or(anyhow!("no parent id"))?;
                    let inputs = vec![
                        format!("{}u128", parent_id),
                        format!("{}u128", mov.node_id),
                        format!("{}u128", game_state.raw()),
                        format!("{}i8", mov.game_status),
                        format!("{}u8", mov.human_move.expect("no human mov")),
                    ];
                    ("move_to_next", inputs)
                }
                Execution::OpenGame => {
                    let node_id = self.open_game_remote()?.node_id;
                    let inputs = vec![format!("{}u128", node_id)];
                    ("open_game", inputs)
                }
            };

            let result = self.pm.execute_program(
                ALEO_CONTRACT.get().unwrap(),
                function,
                inputs.iter(),
                FEE_NUM,
                None,
                None,
            );

            result
        };

        while let Some(exec) = rx.blocking_recv() {
            match handler(exec.clone()) {
                Ok(resp) => tracing::info!("execution result: {:?}", resp),
                Err(e) => tracing::error!("execution {exec:?} error: {:?}", e),
            }
        }

        anyhow::bail!("mori move channel closed")
    }

    pub fn initial(self, rx: Receiver<Execution>) -> Self {
        let self_clone = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = self_clone.execute_program(rx) {
                tracing::error!("execute program error: {:?}", e);
            }
        });

        let self_clone = self.clone();
        std::thread::spawn(move || loop {
            if let Err(e) = self_clone.sync() {
                tracing::error!("sync error: {:?}", e);
            }
            std::thread::sleep(std::time::Duration::from_secs(15));
        });

        self
    }

    pub fn handle_vote(&self, t: Transition<N>) -> anyhow::Result<()> {
        tracing::info!("Got a vote from {}", t.id());
        if let Some(output) = t.outputs().iter().next() {
            if let Some(record) = output.record() {
                if record.1.is_owner(&self.vk) {
                    let (_, record) = record;
                    let record = record.decrypt(&self.vk)?;
                    tracing::info!("Got a vote record {}", record);
                    let vote = Vote::try_from_record(record)?;

                    let node = self.mori_nodes.get(&vote.node_id)?;
                    if let Some(node) = node {
                        let node_id = node.node_id;
                        let mut node = node;

                        if node.check_and_add_vote(vote) {
                            let movs = self.move_to_next_remote(node.clone())?;
                            for mov in movs {
                                self.tx.blocking_send(Execution::MoveToNext(mov))?;
                            }
                        }
                        self.mori_nodes.insert(&node_id, &node)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn handle_open(&self, t: Transition<N>) -> anyhow::Result<()> {
        let input = t.inputs()[0].clone();

        if let Input::Public(_, Some(p)) = input {
            let node_id = handle_u128_plaintext(&p)?;
            let node = self.get_remote_node(node_id)?;
            tracing::info!(
                "Got a new open game id {node_id} node:\n {}",
                node.state.pretty()
            );
            self.mori_nodes.insert(&node_id, &node)?;
        }

        Ok(())
    }

    pub fn handle_move(&self, t: Transition<N>) -> anyhow::Result<()> {
        let inputs = t.inputs();

        let node_id = inputs[1].clone();
        if let Input::Public(_, Some(p)) = node_id {
            let node_id = handle_u128_plaintext(&p)?;
            let node = self.get_remote_node(node_id)?;
            tracing::info!(
                "Got a new move id {node_id} node:\n {}",
                node.state.pretty()
            );
            self.mori_nodes.insert(&node_id, &node)?;
        }

        Ok(())
    }

    pub fn get_remote_node(&self, node_id: u128) -> anyhow::Result<GameNode> {
        let value = self.aleo_client.get_mapping_value(
            ALEO_CONTRACT.get().unwrap(),
            "nodes",
            Plaintext::from_str(&format!("{}u128", node_id))?,
        )?;

        let ai_path = format!("{}/api/nodes/{}", self.ai_dest, node_id);
        let ai_resp = ureq::get(&ai_path)
            .set("Authorization", &self.ai_token)
            .call()?
            .into_json::<RestResponse>()?;

        if let aleo_rust::Value::Plaintext(p) = value {
            let mut node = GameNode::from_plaintext(&p)?;
            node.update_valid_movs(ai_resp.valid_moves);
            Ok(node)
        } else {
            anyhow::bail!("invalid node value")
        }
    }

    pub fn open_game_remote(&self) -> anyhow::Result<RestResponse> {
        let dest = format!("{}/api/nodes", self.ai_dest);
        let node_resp = ureq::post(&dest)
            .set("Authorization", &self.ai_token)
            .call()?
            .into_json()?;
        tracing::info!("open game remote resp {:?}", node_resp);
        Ok(node_resp)
    }

    pub fn move_to_next_remote(&self, node: GameNode) -> anyhow::Result<Vec<RestResponse>> {
        let dest = format!("{}/api/nodes", self.ai_dest);
        let req = MovRequest::from_node(node);

        tracing::info!("move to next req {}", ureq::json!(req));

        let resp: Vec<RestResponse> = ureq::post(&dest)
            .set("Authorization", &self.ai_token)
            .send_json(ureq::json!(req))?
            .into_json()?;
        tracing::info!("move to next resp {:?}", resp);

        // TODO: mov = 64
        let resp = resp
            .into_iter()
            .map(|m| {
                let mut resp = m;
                while resp.is_pass() {
                    tracing::info!("the mov {resp:?} is pass");
                    let req = MovRequest::pass(resp.node_id);
                    if let Ok(r) = ureq::post(&dest)
                        .set("Authorization", &self.ai_token)
                        .send_json(ureq::json!(req))
                    {
                        if let Ok(r) = r.into_json() {
                            resp = r;
                        }
                    }
                }
                resp
            })
            .collect();

        Ok(resp)
    }

    pub fn get_all_nodes(&self) -> anyhow::Result<Vec<(u128, GameNode)>> {
        let nodes = self.mori_nodes.get_all()?;
        Ok(nodes)
    }

    pub fn set_cur_height(&self, height: u32) -> anyhow::Result<()> {
        let cur = self.network_height.get(&self.network_key)?.unwrap_or(0);
        if height > cur {
            self.network_height.insert(&self.network_key, &height)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum Execution {
    MoveToNext(RestResponse),
    OpenGame,
}
