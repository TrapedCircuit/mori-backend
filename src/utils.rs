use std::ops::Deref;

use aleo_rust::{Address, Field, Literal, Network, Plaintext};
use snarkvm::prelude::Entry;

pub fn entry_to_plain<N: Network>(e: &Entry<N, Plaintext<N>>) -> anyhow::Result<&Plaintext<N>> {
    if let Entry::Public(v) = e {
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
