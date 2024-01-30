use std::{ops::Deref, str::FromStr};

use aleo_rust::{Address, Entry, Field, Identifier, Literal, Network, Plaintext};
use anyhow::anyhow;

use crate::cores::NodeEdge;

pub fn entry_to_plain<N: Network>(e: &Entry<N, Plaintext<N>>) -> anyhow::Result<&Plaintext<N>> {
    if let Entry::Private(v) = e {
        Ok(v)
    } else {
        anyhow::bail!("invalid entry")
    }
}

pub fn handle_u8_plaintext<N: Network>(plaintext: &Plaintext<N>) -> anyhow::Result<u8> {
    if let Plaintext::Literal(Literal::U8(v), _) = plaintext {
        Ok(*v.deref())
    } else {
        anyhow::bail!("invalid u32 plaintext")
    }
}

pub fn handle_i8_plaintext<N: Network>(plaintext: &Plaintext<N>) -> anyhow::Result<i8> {
    if let Plaintext::Literal(Literal::I8(v), _) = plaintext {
        Ok(*v.deref())
    } else {
        anyhow::bail!("invalid u32 plaintext")
    }
}

pub fn handle_u128_plaintext<N: Network>(plaintext: &Plaintext<N>) -> anyhow::Result<u128> {
    if let Plaintext::Literal(Literal::U128(v), _) = plaintext {
        Ok(*v.deref())
    } else {
        anyhow::bail!("invalid u128 plaintext")
    }
}

pub fn handle_field_plaintext<N: Network>(plaintext: &Plaintext<N>) -> anyhow::Result<Field<N>> {
    if let Plaintext::Literal(Literal::Field(v), _) = plaintext {
        Ok(*v)
    } else {
        anyhow::bail!("invalid field plaintext")
    }
}

pub fn handle_addr_plaintext<N: Network>(plaintext: &Plaintext<N>) -> anyhow::Result<Address<N>> {
    if let Plaintext::Literal(Literal::Address(v), _) = plaintext {
        Ok(*v)
    } else {
        anyhow::bail!("invalid address plaintext")
    }
}

pub fn handle_from_plaintext<N: Network>(plaintext: &Plaintext<N>) -> anyhow::Result<NodeEdge> {
    if let Plaintext::Struct(s,_) = plaintext {
        let (from_node_id_ident, from_mov_ident) = (
            Identifier::from_str("node_id")?,
            Identifier::from_str("mov")?,
        );

        let (from_node_id_entry, from_mov_entry) = (
            s.get(&from_node_id_ident).ok_or(anyhow!("Invalid record"))?,
            s.get(&from_mov_ident).ok_or(anyhow!("Invalid record"))?,
        );

        let (from_node_id, from_mov) = (
            handle_u128_plaintext(from_node_id_entry)?,
            handle_u8_plaintext(from_mov_entry)?,
        );

        Ok(NodeEdge {
            node_id: from_node_id,
            mov: from_mov,
        })
    } else {
        anyhow::bail!("invalid address plaintext")
    }
}
