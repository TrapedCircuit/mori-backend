use std::{collections::HashMap, str::FromStr};

use aleo_rust::{Identifier, Network, Plaintext, Record};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::utils::{
    entry_to_plain, handle_addr_plaintext, handle_from_plaintext, handle_i8_plaintext,
    handle_u128_plaintext, handle_u8_plaintext,
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
                1 => 0b01,
                -1 => 0b10,
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
                0b01 => result.push(1),
                0b10 => result.push(-1),
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
    pub from: NodeEdge,
    pub game_status: i8,

    pub valid_movs: Vec<u8>,
    pub votes: Vec<Vote>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeEdge {
    pub node_id: u128,
    pub mov: u8,
}

impl GameNode {
    pub fn check_and_add_vote(&mut self, vote: Vote) -> bool {
        // check mov valid
        if !self.valid_movs.contains(&vote.mov) {
            return false;
        }
        self.votes.push(vote);
        self.game_status == 0
    }

    pub fn update_valid_movs(&mut self, movs: Vec<u8>) {
        self.valid_movs = movs;
    }

    pub fn is_root(&self) -> bool {
        self.from.node_id == 0
    }

    pub fn from_plaintext<N: Network>(p: &Plaintext<N>) -> anyhow::Result<Self> {
        let (node_id_ident, state_ident, from_ident, game_status_ident) = (
            Identifier::from_str("node_id")?,
            Identifier::from_str("state")?,
            Identifier::from_str("from")?,
            Identifier::from_str("game_status")?,
        );
        const ERR: &str = "Invalid record";
        if let Plaintext::Struct(s, _) = p {
            let (node_id_entry, state_entry, from_entry, game_status_entry) = (
                s.get(&node_id_ident).ok_or(anyhow!(ERR))?,
                s.get(&state_ident).ok_or(anyhow!(ERR))?,
                s.get(&from_ident).ok_or(anyhow!(ERR))?,
                s.get(&game_status_ident).ok_or(anyhow!(ERR))?,
            );

            let (node_id, state, from, game_status) = (
                handle_u128_plaintext(node_id_entry)?,
                handle_u128_plaintext(state_entry)?,
                handle_from_plaintext(from_entry)?,
                handle_i8_plaintext(game_status_entry)?,
            );
            Ok(Self {
                node_id,
                state: GameState(state),
                from,
                game_status,
                valid_movs: vec![],
                votes: vec![],
            })
        } else {
            anyhow::bail!("Invalid record")
        }
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
