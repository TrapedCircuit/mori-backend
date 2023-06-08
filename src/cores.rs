use std::str::FromStr;

use aleo_rust::{Identifier, Network, Plaintext, Record};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::utils::{
    entry_to_plain, handle_addr_plaintext, handle_field_plaintext, handle_u8_plaintext,
};

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct GameState(u128);

impl std::fmt::Debug for GameState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameState")
            .field("inner", &self.pretty())
            .finish()
    }
}

impl GameState {
    fn pretty(&self) -> String {
        // two bits per square
        let mut result = String::new();
        for i in 0..64 {
            let square = (self.0 >> (2 * i)) & 0b11;
            match square {
                0b00 => result.push_str(" ."),
                0b01 => result.push_str(" B"),
                0b10 => result.push_str(" W"),
                _ => panic!("Invalid square value"),
            }
            if i % 8 == 7 {
                result.push('\n');
            }
        }
        result
    }

    pub fn zero() -> Self {
        // e4 d5 Black
        // d4 e5 White
        GameState(7083711853891053158400)
    }

    pub fn check_pos_valid(&self, pos: u8) -> bool {
        if pos > 63 {
            return false;
        }
        let square = (self.0 >> (2 * pos)) & 0b11;
        if square == 0b00 {
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub sender: String,
    pub node_id: String,
    pub mov: u8,
}

impl Vote {
    pub fn try_from_record<N: Network>(record: Record<N, Plaintext<N>>) -> anyhow::Result<Self> {
        let (sender_ident, node_id_ident, mov_ident) = (
            Identifier::from_str("sender")?,
            Identifier::from_str("node_id")?,
            Identifier::from_str("mov")?,
        );
        const ERR: &str = "Invalid record";
        let (sender_entry, node_id_entry, mov_entry) = (
            record.data().get(&sender_ident).ok_or(anyhow!(ERR))?,
            record.data().get(&node_id_ident).ok_or(anyhow!(ERR))?,
            record.data().get(&mov_ident).ok_or(anyhow!(ERR))?,
        );

        let (sender, node_id, mov) = (
            handle_addr_plaintext(entry_to_plain(sender_entry)?)?,
            handle_field_plaintext(entry_to_plain(node_id_entry)?)?,
            handle_u8_plaintext(entry_to_plain(mov_entry)?)?,
        );

        Ok(Self {
            sender: sender.to_string(),
            node_id: node_id.to_string(),
            mov,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameNode {
    pub node_id: String,
    pub state: GameState,
    pub parent_id: String,
    pub node_type: u8,
    pub game_status: u8,

    pub valid_mov_cnt: u8,
    pub votes: Vec<Vote>,
}

impl GameNode {
    pub fn add_vote(&mut self, vote: Vote) -> bool {
        let vote_len = (self.votes.len() + 1) as u8;
        if vote_len <= self.valid_mov_cnt / 2 {
            self.votes.push(vote);
        }

        vote_len >= self.valid_mov_cnt / 2
    }
}

impl FromStr for GameNode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        //"{\n  node_id: 25939613864655546608765990709996941590field,\n  state: 7083711853891053158400u128,\n  parent_id: 6291506693248725078901406641668230701639003122863888065466938556813880264799field,\n  node_type: 0u8,\n  game_status: 0u8\n}"

        let s = s
            .replace("node_id: ", "\"node_id\": \"")
            .replace("state: ", "\"state\": \"")
            .replace("parent_id: ", "\"parent_id\": \"")
            .replace("node_type", "\"node_type\"")
            .replace("game_status", "\"game_status\"")
            .replace("field", "field\"")
            .replace("u128", "\"")
            .replace("u8", "")
            .replace("\n", "");

        let game_node_value = serde_json::from_str::<serde_json::Value>(&s)?;
        let node_id = game_node_value["node_id"]
            .as_str()
            .ok_or(anyhow!("Invalid node_id"))?;
        let state = game_node_value["state"]
            .as_str()
            .ok_or(anyhow!("Invalid state"))?;
        let parent_id = game_node_value["parent_id"]
            .as_str()
            .ok_or(anyhow!("Invalid parent_id"))?;
        let node_type = game_node_value["node_type"]
            .as_u64()
            .ok_or(anyhow!("Invalid node_type"))?;
        let game_status = game_node_value["game_status"]
            .as_u64()
            .ok_or(anyhow!("Invalid game_status"))?;

        // TODO: remove this mock
        let mock_valid = 4;
        let votes = vec![];

        Ok(Self {
            node_id: node_id.to_string(),
            state: GameState(state.parse::<u128>()?),
            parent_id: parent_id.to_string(),
            node_type: node_type as u8,
            game_status: game_status as u8,
            valid_mov_cnt: mock_valid,
            votes,
        })
    }
}

#[test]
fn test_game_node_from_str() {
    let game_node_str = "{\n  node_id: 25939613864655546608765990709996941590field,\n  state: 7083711853891053158400u128,\n  parent_id: 6291506693248725078901406641668230701639003122863888065466938556813880264799field,\n  node_type: 0u8,\n  game_status: 0u8\n}";
    let game_node = GameNode::from_str(game_node_str).unwrap();
    println!("{:?}", game_node);
}
