use std::sync::Arc;

use once_cell::sync::OnceCell;
use serde::{de::DeserializeOwned, Serialize};

const DB_PATH: &str = "./mori_db";

#[derive(Clone)]
pub struct RocksDB(Arc<rocksdb::DB>);

impl RocksDB {
    pub fn open() -> anyhow::Result<Self> {
        static DB: OnceCell<RocksDB> = OnceCell::new();

        // Retrieve the database.
        let database = DB
            .get_or_try_init(|| {
                // Customize database options.
                let mut options = rocksdb::Options::default();
                options.set_compression_type(rocksdb::DBCompressionType::Lz4);
                let rocksdb = {
                    options.increase_parallelism(2);
                    options.create_if_missing(true);

                    Arc::new(rocksdb::DB::open(&options, DB_PATH)?)
                };

                Ok::<_, anyhow::Error>(RocksDB(rocksdb))
            })?
            .clone();

        Ok(database)
    }

    pub fn open_map<K: Serialize + DeserializeOwned, V: Serialize + DeserializeOwned>(
        prefix: &str,
    ) -> anyhow::Result<DBMap<K, V>> {
        let db = Self::open()?;

        let prefix = prefix.as_bytes().to_vec();

        Ok(DBMap {
            inner: db.inner(),
            prefix,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn inner(&self) -> Arc<rocksdb::DB> {
        self.0.clone()
    }
}

#[derive(Clone)]
pub struct DBMap<K: Serialize + DeserializeOwned, V: Serialize + DeserializeOwned> {
    pub inner: Arc<rocksdb::DB>,
    prefix: Vec<u8>,
    _marker: std::marker::PhantomData<(K, V)>,
}

impl<K: Serialize + DeserializeOwned, V: Serialize + DeserializeOwned> DBMap<K, V> {
    pub fn insert(&self, key: K, value: V) -> anyhow::Result<()> {
        let key_bytes = bincode::serialize(&key)?;
        let value_bytes = bincode::serialize(&value)?;

        let real_key = [self.prefix.clone(), key_bytes].concat();

        self.inner.put(real_key, value_bytes)?;

        Ok(())
    }

    pub fn batch_insert(&self, kvs: Vec<(K, V)>) -> anyhow::Result<()> {
        let mut batch = rocksdb::WriteBatch::default();

        for (key, value) in kvs {
            let key_bytes = bincode::serialize(&key)?;
            let value_bytes = bincode::serialize(&value)?;

            let real_key = [self.prefix.clone(), key_bytes].concat();

            batch.put(real_key, value_bytes);
        }

        self.inner.write(batch)?;

        Ok(())
    }

    pub fn remove(&self, key: &K) -> anyhow::Result<()> {
        let key_bytes = bincode::serialize(&key)?;
        let real_key = [self.prefix.clone(), key_bytes].concat();

        self.inner.delete(real_key)?;

        Ok(())
    }

    pub fn batch_remove(&self, keys: &Vec<K>) -> anyhow::Result<()> {
        let mut batch = rocksdb::WriteBatch::default();

        for key in keys {
            let key_bytes = bincode::serialize(key)?;
            let real_key = [self.prefix.clone(), key_bytes].concat();

            batch.delete(real_key);
        }

        self.inner.write(batch)?;

        Ok(())
    }

    pub fn get_all(&self) -> anyhow::Result<Vec<(K, V)>> {
        let mut result = Vec::new();

        let iter = self.inner.prefix_iterator(self.prefix.clone());

        for item in iter {
            let (key, value) = item?;
            let key = &key[self.prefix.len()..];
            let key = bincode::deserialize(key)?;
            let value = bincode::deserialize(&value)?;

            result.push((key, value));
        }

        Ok(result)
    }

    pub fn get(&self, key: &K) -> anyhow::Result<Option<V>> {
        let key_bytes = bincode::serialize(key)?;
        let real_key = [self.prefix.clone(), key_bytes].concat();

        let value = self.inner.get(real_key)?;

        if let Some(value) = value {
            let value = bincode::deserialize(&value)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn pop_front(&self) -> anyhow::Result<Option<(K, V)>> {
        let mut iter = self.inner.prefix_iterator(self.prefix.clone());

        if let Some(item) = iter.next() {
            let (key, value) = item?;
            let key = &key[self.prefix.len()..];
            let key = bincode::deserialize(key)?;
            let value = bincode::deserialize(&value)?;

            self.remove(&key)?;

            Ok(Some((key, value)))
        } else {
            Ok(None)
        }
    }

    pub fn contain(&self, key: &K) -> anyhow::Result<bool> {
        let key_bytes = bincode::serialize(key)?;
        let real_key = [self.prefix.clone(), key_bytes].concat();

        let value = self.inner.get(real_key)?;

        Ok(value.is_some())
    }
}

#[test]
fn test_rocksdb() {
    let map = RocksDB::open_map::<String, String>("test").unwrap();

    let (key1, value1) = ("key1".to_string(), "value1".to_string());
    let (key2, value2) = ("key2".to_string(), "value2".to_string());
    let (key3, value3) = ("key3".to_string(), "value3".to_string());

    map.insert(key1.clone(), value1.clone()).unwrap();
    map.insert(key2.clone(), value2.clone()).unwrap();
    map.insert(key3.clone(), value3.clone()).unwrap();

    let all = map.get_all().unwrap();

    assert_eq!(all.len(), 3);
    assert_eq!(all[0], (key1, value1));
    assert_eq!(all[1], (key2, value2));
    assert_eq!(all[2], (key3, value3));
}

#[test]
fn test_batch_op() {
    let map = RocksDB::open_map::<String, String>("test").unwrap();

    let (key1, value1) = ("key1".to_string(), "value1".to_string());
    let (key2, value2) = ("key2".to_string(), "value2".to_string());

    map.batch_insert(vec![
        (key1.clone(), value1.clone()),
        (key2.clone(), value2.clone()),
    ])
    .unwrap();

    let all = map.get_all().unwrap();

    assert_eq!(all.len(), 2);
    assert_eq!(all[0], (key1.clone(), value1));
    assert_eq!(all[1], (key2.clone(), value2));

    map.batch_remove(&vec![key1, key2]).unwrap();

    let all = map.get_all().unwrap();

    assert_eq!(all.len(), 0);
}
