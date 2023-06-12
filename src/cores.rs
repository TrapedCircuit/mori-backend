use std::{collections::HashMap, str::FromStr};

use aleo_rust::{Identifier, Network, Plaintext, Record};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::utils::{
    entry_to_plain, handle_addr_plaintext, handle_u128_plaintext, handle_u8_plaintext,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct GameState(u128);

impl GameState {
    pub fn pretty(&self) -> String {
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

    pub fn raw(&self) -> u128 {
        self.0
    }

    pub fn from_vec_i8(vec: &[i8]) -> Self {
        let mut result = 0;
        for i in 0..64 {
            let square = match vec[i] {
                0 => 0b00,
                -1 => 0b10,
                1 => 0b01,
                _ => panic!("Invalid square value"),
            };
            result |= square << (2 * i);
        }
        GameState(result)
    }

    pub fn to_vec_i8(&self) -> Vec<i8> {
        let mut result = Vec::new();
        for i in 0..64 {
            let square = (self.0 >> (2 * i)) & 0b11;
            match square {
                0b00 => result.push(0),
                0b01 => result.push(-1),
                0b10 => result.push(1),
                _ => panic!("Invalid square value"),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vote {
    pub sender: String,
    pub node_id: u128,
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
            handle_u128_plaintext(entry_to_plain(node_id_entry)?)?,
            handle_u8_plaintext(entry_to_plain(mov_entry)?)?,
        );

        Ok(Self {
            sender: sender.to_string(),
            node_id,
            mov,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameNode {
    pub node_id: u128,
    pub state: GameState,
    pub parent_id: u128,
    pub node_type: u8,
    pub game_status: u8,
    pub valid_cnt: u32,

    pub valid_movs: Vec<u8>,
    pub votes: Vec<Vote>,
}

impl GameNode {
    pub fn check_and_add_vote(&mut self, vote: Vote) -> bool {
        // check mov valid
        if !self.valid_movs.contains(&vote.mov) {
            return false;
        }
        // check vote len
        let vote_len = (self.votes.len() + 1) as u32;
        if vote_len <= self.valid_cnt / 2 {
            self.votes.push(vote);
        }

        (vote_len >= self.valid_cnt / 2) && self.game_status == 0 && self.node_type == 0
    }
    pub fn update_valid_movs(&mut self, movs: Vec<u8>) {
        self.valid_movs = movs;
    }
}

impl FromStr for GameNode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s
            .replace("node_id: ", "\"node_id\": \"")
            .replace("state: ", "\"state\": \"")
            .replace("parent_id: ", "\"parent_id\": \"")
            .replace("node_type", "\"node_type\"")
            .replace("game_status", "\"game_status\"")
            .replace("valid_cnt", "\"valid_cnt\"")
            .replace("u128", "\"")
            .replace("u32", "")
            .replace("u8", "")
            .replace("i8", "")
            .replace("\\n", ""); //TODO: use a better way to handle it

        println!("s: {}", s);

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
        let valid_cnt = game_node_value["valid_cnt"]
            .as_u64()
            .ok_or(anyhow!("Invalid valid_cnt"))?;

        let votes = vec![];
        let valid_movs = vec![];

        Ok(Self {
            node_id: node_id.parse::<u128>()?,
            state: GameState(state.parse::<u128>()?),
            parent_id: parent_id.parse::<u128>()?,
            node_type: node_type as u8,
            game_status: game_status as u8,
            valid_cnt: valid_cnt as u32,
            valid_movs,
            votes,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestResponse {
    #[serde(rename = "id")]
    pub node_id: u128,

    #[serde(rename = "parentId")]
    pub parent_id: Option<u128>,

    #[serde(rename = "type")]
    pub node_type: u8,

    pub state: Vec<i8>,

    #[serde(rename = "validMoves")]
    pub valid_moves: Vec<u8>,

    #[serde(rename = "result")]
    pub game_status: i8,

    #[serde(rename = "humanMove")]
    pub human_move: Option<u8>,

    #[serde(rename = "aiMove")]
    pub ai_move: Option<u8>,
}

impl RestResponse {
    pub fn is_pass(&self) -> bool {
        self.valid_moves == vec![64]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovRequest {
    #[serde(rename = "parentId")]
    parent_id: u128,

    votes: Vec<Votes>,
}

impl MovRequest {
    pub fn pass(node_id: u128) -> Self {
        Self {
            parent_id: node_id,
            votes: vec![Votes {
                mov: 64,
                addresses: vec![],
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Votes {
    #[serde(rename = "move")]
    mov: u8,
    addresses: Vec<String>,
}

impl MovRequest {
    pub fn from_node(node: GameNode) -> MovRequest {
        let mut votes_map = HashMap::new();

        for (idx, v) in node.votes.iter().enumerate() {
            if idx == 0 {
                votes_map.insert(
                    v.mov,
                    Votes {
                        mov: v.mov,
                        addresses: vec![v.sender.clone()],
                    },
                );
                continue;
            }

            match votes_map.get_mut(&v.mov) {
                Some(votes) => {
                    votes.addresses.push(v.sender.clone());
                }
                None => {
                    votes_map.insert(
                        v.mov,
                        Votes {
                            mov: v.mov,
                            addresses: vec![v.sender.clone()],
                        },
                    );
                }
            }
        }

        Self {
            parent_id: node.node_id,
            votes: votes_map.values().cloned().collect(),
        }
    }
}

#[test]
fn test_game_node_from_str() {
    let game_node_str = "\"{\n  node_id: 1u128,\n  state: 7083711853891053158400u128,\n  parent_id: 0u128,\n  node_type: 0u8,\n  game_status: 0i8,\n  valid_cnt: 4u32\n}\"";
    let game_node_str = game_node_str.trim_matches('\"');

    let game_node = GameNode::from_str(game_node_str).unwrap();

    println!("{:?}", game_node);
}

#[test]
fn test_game_state_from_vec_i8() {
    let game_state_vec_i8 = vec![
        0, 0, 0, 0, 0, 0, 0, 0, -1, 0, 0, 0, 0, 0, 0, 0, -1, 0, 1, 0, 0, 0, 0, 0, -1, 0, 1, 0, 0,
        0, 0, 0, -1, 0, 1, 0, 0, 0, 0, 0, -1, 0, 1, 0, 0, 0, 0, 0, -1, 0, 1, 0, 0, 0, 0, 0, -1, 0,
        1, 0, 0, 0, 0, 0,
    ];

    let game_state = GameState::from_vec_i8(&game_state_vec_i8);
    let to_vec = game_state.to_vec_i8();
    assert_eq!(game_state_vec_i8, to_vec);

    println!("{}", game_state.pretty());
}

#[test]
fn test_game_state_pretty() {
    let game_state = GameState(7083711853891053158400);
    println!("{}", game_state.pretty());
    let game_state = GameState(6198286153301259976704);
    println!("{}", game_state.pretty());
    let game_state = GameState(6198295159950758903808);
    println!("{}", game_state.pretty());
}

#[test]
fn test_game_node_to_req() {
    let game_node_str = "\"{\n  node_id: 1u128,\n  state: 7083711853891053158400u128,\n  parent_id: 0u128,\n  node_type: 0u8,\n  game_status: 0i8,\n  valid_cnt: 4u32\n}\"";
    let game_node_str = game_node_str.trim_matches('\"');

    let mut game_node = GameNode::from_str(game_node_str).unwrap();

    let vote1 = Vote {
        mov: 26,
        sender: "0x123".to_string(),
        node_id: 1,
    };

    let vote2 = Vote {
        mov: 19,
        sender: "0x456".to_string(),
        node_id: 1,
    };

    game_node.check_and_add_vote(vote1);
    game_node.check_and_add_vote(vote2);

    let mov_req = MovRequest::from_node(game_node);
    println!("{:?}", mov_req);
}
