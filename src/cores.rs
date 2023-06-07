use std::str::FromStr;

use aleo_rust::{Identifier, Network, Plaintext, Record};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::utils::{handle_addr_plaintext, handle_field_plaintext, handle_u8_plaintext, entry_to_plain};

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
