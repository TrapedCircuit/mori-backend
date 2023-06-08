use cores::{GameNode, Vote};
use rand::Rng;
use snarkvm::synthesizer::{Input, Transition};
use std::str::FromStr;
use tokio::sync::mpsc::{Receiver, Sender};
use utils::handle_field_plaintext;

use aleo_rust::{
    AleoAPIClient, Ciphertext, Field, Network, Plaintext, PrivateKey, ProgramID, ProgramManager,
    Record, ViewKey,
};
use db::{DBMap, RocksDB};
use filter::TransitionFilter;

pub mod cores;
pub mod db;
pub mod filter;
pub mod utils;

const ALEO_NETWORK: &str = "testnet3";

#[derive(Clone)]
pub struct Mori<N: Network> {
    pm: ProgramManager<N>,
    aleo_client: AleoAPIClient<N>,
    filter: TransitionFilter<N>,
    pub tx: Sender<Execution>,

    ai_dest: String,
    aleo_rpc: String,

    pk: PrivateKey<N>,
    vk: ViewKey<N>,
    network_key: String, // <dest>-<pk>

    network_height: DBMap<String, u32>,
    unspent_records: DBMap<String, Record<N, Plaintext<N>>>, // for execution gas
    mori_nodes: DBMap<String, GameNode>,                     // <node_id, node>
}

impl<N: Network> Mori<N> {
    pub fn new(
        aleo_rpc: Option<String>,
        pk: PrivateKey<N>,
        tx: Sender<Execution>,
        ai_dest: String,
    ) -> anyhow::Result<Self> {
        let aleo_rpc = aleo_rpc.unwrap_or("https://vm.aleo.org/api".to_string());
        let aleo_client = AleoAPIClient::new(&aleo_rpc, ALEO_NETWORK)?;
        let network_key = format!("{}-{}", aleo_rpc, pk);

        let vk = ViewKey::try_from(&pk)?;
        let pm = ProgramManager::new(Some(pk), None, Some(aleo_client.clone()), None)?;
        let filter = TransitionFilter::new()
            .add_program(ProgramID::from_str("mori.aleo")?)
            .add_function("vote".to_string());

        let unspent_records = RocksDB::open_map("unspent_records")?;
        let mori_nodes = RocksDB::open_map("mori_nodes")?;
        let network_height = RocksDB::open_map("network")?;

        Ok(Self {
            pm,
            aleo_client,
            filter,

            ai_dest,
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
        tracing::info!("syncing aleo block from {} to {}", cur, latest);
        const BATCH_SIZE: usize = 45;

        for start in (cur..latest).step_by(BATCH_SIZE) {
            let end = (start + BATCH_SIZE as u32).min(latest);
            tracing::warn!("fetching aleo blocks from {} to {}", start, end);
            let transitions = self
                .aleo_client
                .get_blocks(start, end)?
                .into_iter()
                .flat_map(|b| self.filter.filter_block(b))
                .collect::<Vec<Transition<N>>>();
            let credits_id = ProgramID::<N>::from_str("credits.aleo")?;
            let mori_id = ProgramID::<N>::from_str("mori.aleo")?;

            for t in transitions {
                match (t.function_name().to_string().as_str(), t.program_id()) {
                    ("vote", pid) if *pid == mori_id => self.handle_vote(t)?,
                    ("move_to_next", pid) if *pid == mori_id => self.handle_move(t)?,
                    ("open_game", pid) if *pid == mori_id => self.handle_open(t)?,
                    (_, pid) if *pid == credits_id => self.handle_credits(t)?,
                    _ => {}
                }
            }
        }
        self.network_height.insert(&self.network_key, &latest)?;
        tracing::info!("synced aleo block from {} to {}", cur, latest);
        Ok(())
    }

    pub fn execute_program(mut self, mut rx: Receiver<Execution>) -> anyhow::Result<()> {
        // TODO: error handling
        while let Some(exec) = rx.blocking_recv() {
            tracing::warn!("received execution: {:?}", exec);
            let (function, inputs) = match exec {
                Execution::MoveToNext(vote) => {
                    let Vote {
                        mov: _,
                        sender: _,
                        node_id,
                    } = vote;
                    let mut rng = rand::thread_rng();
                    let new_node_id = Field::<N>::from_u128(rng.gen());
                    // TODO: replace real get new state
                    let mock_new_state: u128 = rng.gen();
                    let mock_valid_pos: u32 = rng.gen_range(0..=3);
                    let mock_game_status: u8 = rng.gen_range(0..=3);

                    let inputs = vec![
                        format!("{}field", node_id),
                        format!("{}field", new_node_id),
                        format!("{}u128", mock_new_state),
                        format!("{}u32", mock_valid_pos),
                        format!("{}u8", mock_game_status),
                    ];
                    ("move_to_next", inputs)
                }
                Execution::OpenGame => {
                    let mut rng = rand::thread_rng();
                    let node_id = Field::<N>::from_u128(rng.gen());
                    let inputs = vec![format!("{}field", node_id)];
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
            );
            match result {
                Ok(result) => tracing::info!("move_to_next result: {:?}", result),
                Err(e) => tracing::error!("move_to_next error: {:?}", e),
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

    pub fn handle_credits(&self, t: Transition<N>) -> anyhow::Result<()> {
        tracing::info!("got a new credits transition {:?}", t);
        for output in t.outputs() {
            if let Some(record) = output.record() {
                if record.1.is_owner(&self.vk) {
                    let (commitment, record) = record;
                    let sn = Record::<N, Ciphertext<N>>::serial_number(
                        self.pk,
                        *commitment,
                    )?;
                    let record = record.decrypt(&self.vk)?;
                    tracing::info!("got a new record {:?}", record);
                    self.unspent_records.insert(&sn.to_string(), &record)?;
                }
            }
        }

        Ok(())
    }

    pub fn handle_vote(&self, t: Transition<N>) -> anyhow::Result<()> {
        if let Some(output) = t.outputs().iter().next() {
            if let Some(record) = output.record() {
                if record.1.is_owner(&self.vk) {
                    let (_, record) = record;
                    let record = record.decrypt(&self.vk)?;
                    let vote = Vote::try_from_record(record)?;

                    let node = self.mori_nodes.get(&vote.node_id)?;
                    if let Some(node) = node {
                        let node_id = node.node_id.clone();
                        let mut node = node;
                        if node.add_vote(vote.clone()) {
                            self.tx.blocking_send(Execution::MoveToNext(vote))?;
                        }

                        self.mori_nodes.insert(&node_id, &node)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn handle_open(&self, t: Transition<N>) -> anyhow::Result<()> {
        let node_id_input = t.inputs().iter().next();
        if let Some(node_id_input) = node_id_input {
            if let Input::Public(_, Some(node_id)) = node_id_input {
                let node_id = handle_field_plaintext(node_id)?;
                tracing::info!("got a new node id {:?}", node_id);
                // TODO: ureq request validator
                let node = self.get_remote_node(node_id.to_string())?;
                self.mori_nodes.insert(&node_id.to_string(), &node)?;
            }
        }

        Ok(())
    }

    pub fn handle_move(&self, t: Transition<N>) -> anyhow::Result<()> {
        let node_id_input = t.inputs().get(1);
        if let Some(node_id_input) = node_id_input {
            if let Input::Public(_, Some(node_id)) = node_id_input {
                let node_id = handle_field_plaintext(node_id)?;
                tracing::info!("got a new node id {:?}", node_id);
                // TODO: ureq request validator
                let node = self.get_remote_node(node_id.to_string())?;
                self.mori_nodes.insert(&node_id.to_string(), &node)?;
            }
        }

        Ok(())
    }

    pub fn get_remote_node(&self, node_id: String) -> anyhow::Result<GameNode> {
        let path = format!("{}/mori/node/{}", self.aleo_rpc, node_id);
        let resp = ureq::get(&path).call()?.into_json::<GameNode>()?;
        Ok(resp)
    }

    pub fn get_all_nodes(&self) -> anyhow::Result<Vec<(String, GameNode)>> {
        let nodes = self.mori_nodes.get_all()?;
        Ok(nodes)
    }

    pub fn set_cur_height(&self, height: u32) -> anyhow::Result<()> {
        self.network_height.insert(&self.network_key.clone(), &height)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum Execution {
    MoveToNext(Vote),
    OpenGame,
}
