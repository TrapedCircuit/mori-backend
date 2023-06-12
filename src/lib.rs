use anyhow::anyhow;
use cores::{GameNode, MovRequest, RestResponse, Vote};
use snarkvm::synthesizer::Transition;
use std::str::FromStr;
use tokio::sync::mpsc::{Receiver, Sender};

use aleo_rust::{
    AleoAPIClient, Block, Ciphertext, Credits, Network, Plaintext, PrivateKey, ProgramID,
    ProgramManager, Record, ViewKey,
};
use db::{DBMap, RocksDB};
use filter::TransitionFilter;

use crate::{cores::GameState, utils::handle_u128_plaintext};

pub mod cores;
pub mod db;
pub mod filter;
pub mod utils;

pub const ALEO_NETWORK: &str = "testnet3";

#[derive(Clone)]
pub struct Mori<N: Network> {
    pm: ProgramManager<N>,
    aleo_client: AleoAPIClient<N>,
    filter: TransitionFilter<N>,
    pub tx: Sender<Execution>,

    ai_dest: String,
    ai_token: String,
    aleo_rpc: String,

    pk: PrivateKey<N>,
    vk: ViewKey<N>,
    network_key: String, // <dest>-<pk>

    network_height: DBMap<String, u32>,
    unspent_records: DBMap<String, Record<N, Plaintext<N>>>, // for execution gas
    mori_nodes: DBMap<u128, GameNode>,                       // <node_id, node>
}

impl<N: Network> Mori<N> {
    pub fn new(
        aleo_rpc: Option<String>,
        pk: PrivateKey<N>,
        tx: Sender<Execution>,
        ai_dest: String,
        ai_token: String,
    ) -> anyhow::Result<Self> {
        let aleo_rpc = aleo_rpc.unwrap_or("https://vm.aleo.org/api".to_string());
        let aleo_client = AleoAPIClient::new(&aleo_rpc, ALEO_NETWORK)?;
        let network_key = format!("{}-{}", aleo_rpc, pk);
        tracing::info!("your private key is: {pk}, network key is {network_key}");

        let vk = ViewKey::try_from(&pk)?;
        let pm = ProgramManager::new(Some(pk), None, Some(aleo_client.clone()), None)?;
        let filter = TransitionFilter::new().add_program(ProgramID::from_str("mori.aleo")?);

        let unspent_records: DBMap<String, Record<N, Plaintext<N>>> =
            RocksDB::open_map("unspent_records")?;
        let mori_nodes = RocksDB::open_map("mori_nodes")?;
        let network_height = RocksDB::open_map("network")?;

        let ai_token = format!(" Bearar {}", ai_token);

        Ok(Self {
            pm,
            aleo_client,
            filter,

            ai_dest,
            ai_token,
            aleo_rpc,

            tx,
            pk,
            vk,
            unspent_records,
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
                .flat_map(|b| {
                    if let Err(e) = self.handle_credits(&b) {
                        tracing::error!("handle credits error: {:?}", e);
                    }
                    self.filter.filter_block(b)
                })
                .collect::<Vec<Transition<N>>>();
            if let Err(e) = ts_handler(transitions) {
                tracing::error!("handle transitions error: {:?}", e);
            }
        }

        self.network_height.insert(&self.network_key, &latest)?;
        tracing::info!("Synced aleo blocks from {} to {}", cur, latest);
        Ok(())
    }

    pub fn execute_program(mut self, mut rx: Receiver<Execution>) -> anyhow::Result<()> {
        let mut handler = move |exec| {
            tracing::warn!("received execution: {:?}", exec);
            let (function, inputs) = match exec {
                Execution::MoveToNext(mov) => {
                    let game_state = GameState::from_vec_i8(&mov.state);
                    let parent_id = mov.parent_id.ok_or(anyhow!("no parent id"))?;
                    let inputs = vec![
                        format!("{}u128", parent_id),
                        format!("{}u128", mov.node_id),
                        format!("{}u128", game_state.raw()),
                        format!("{}u32", mov.valid_moves.len()),
                        format!("{}i8", mov.game_status),
                    ];
                    ("move_to_next", inputs)
                }
                Execution::OpenGame => {
                    let node_id = self.open_game_remote()?.node_id;
                    let inputs = vec![format!("{}u128", node_id)];
                    ("open_game", inputs)
                }
            };

            let (_, fee_record) = self
                .unspent_records
                .pop_front()?
                .ok_or(anyhow::anyhow!("no unspent record for execution gas"))?;

            let result = self.pm.execute_program(
                "mori.aleo",
                function,
                inputs.iter(),
                40000,
                fee_record,
                None,
            )?;

            Ok::<String, anyhow::Error>(result)
        };

        while let Some(exec) = rx.blocking_recv() {
            match handler(exec) {
                Ok(resp) => tracing::info!("execution result: {:?}", resp),
                Err(e) => tracing::error!("execution error: {:?}", e),
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
            tracing::info!("Holded Records {:?}", self_clone.unspent_records.get_all());
            std::thread::sleep(std::time::Duration::from_secs(15));
        });

        self
    }

    pub fn handle_credits(&self, block: &Block<N>) -> anyhow::Result<()> {
        // handle in
        block.clone().into_serial_numbers().for_each(|sn| {
            let _ = self.unspent_records.remove(&sn.to_string());
        });
        // handle out
        for (commit, record) in block.clone().into_records() {
            if !record.is_owner(&self.vk) {
                continue;
            }
            let sn = Record::<N, Ciphertext<N>>::serial_number(self.pk, commit)?;
            let record = record.decrypt(&self.vk)?;
            if let Ok(credits) = record.microcredits() {
                if credits > 40000 {
                    tracing::info!("got a new record {:?}", record);
                    self.unspent_records.insert(&sn.to_string(), &record)?;
                }
            }
        }

        Ok(())
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

                        if node.check_and_add_vote(vote.clone()) {
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
        if let Some(finalizes) = t.finalize() {
            let node_id_final = finalizes.iter().next();
            tracing::info!("Got a new open game transition: {:?}", node_id_final);
            if let Some(aleo_rust::Value::Plaintext(node_id)) = node_id_final {
                let node_id = handle_u128_plaintext(node_id)?;
                let node = self.get_remote_node(node_id)?;
                tracing::info!("Got a new open game node: {:?}", node);
                self.mori_nodes.insert(&node_id, &node)?;
            }
        }

        Ok(())
    }

    pub fn handle_move(&self, t: Transition<N>) -> anyhow::Result<()> {
        if let Some(finalizes) = t.finalize() {
            let parent_id_final = finalizes.get(0);
            let node_id_final = finalizes.get(1);
            tracing::info!("Got a new move transition: {:?}", node_id_final);
            // update new_node_id
            if let Some(aleo_rust::Value::Plaintext(node_id)) = node_id_final {
                let node_id = handle_u128_plaintext(node_id)?;
                let node = self.get_remote_node(node_id)?;
                tracing::info!("Got a new move node: {:?}", node);
                self.mori_nodes.insert(&node_id, &node)?;
            }
            // update parent_id
            if let Some(aleo_rust::Value::Plaintext(parent_id)) = parent_id_final {
                let parent_id = handle_u128_plaintext(parent_id)?;
                let node = self.mori_nodes.get(&parent_id)?;
                if let Some(node) = node {
                    let mut node = node;
                    // to internal node
                    node.node_type = 1;
                    self.mori_nodes.insert(&parent_id, &node)?;
                }
            }
        }

        Ok(())
    }

    pub fn get_remote_node(&self, node_id: u128) -> anyhow::Result<GameNode> {
        let path = format!("{}/testnet3/mori/node/{}", self.aleo_rpc, node_id);
        let resp = ureq::get(&path).call()?.into_string()?;
        let node_str = resp.trim_matches('\"');
        let node = GameNode::from_str(node_str)?;
        Ok(node)
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

        let resp = ureq::post(&dest)
            .set("Authorization", &self.ai_token)
            .send_json(ureq::json!(req))?
            .into_json()?;
        tracing::info!("move to next resp {:?}", resp);
        // TODO: handle mov 64

        Ok(resp)
    }

    pub fn get_all_nodes(&self) -> anyhow::Result<Vec<(u128, GameNode)>> {
        let nodes = self.mori_nodes.get_all()?;
        Ok(nodes)
    }

    pub fn get_all_record(&self) -> anyhow::Result<Vec<(String, Record<N, Plaintext<N>>)>> {
        let records = self.unspent_records.get_all()?;
        Ok(records)
    }

    pub fn set_cur_height(&self, height: u32) -> anyhow::Result<()> {
        // TODO
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
