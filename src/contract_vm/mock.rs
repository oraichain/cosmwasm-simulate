use std::collections::BTreeMap;
#[cfg(feature = "iterator")]
use std::{
    iter,
    ops::{Bound, RangeBounds},
};

use crate::contract_vm::watcher;
use cosmwasm_std::{Binary, CanonicalAddr, Coin, HumanAddr};
use cosmwasm_vm::{Api, Extern, FfiError, FfiResult, Storage};

const GAS_COST_HUMANIZE: u64 = 44;
const GAS_COST_CANONICALIZE: u64 = 55;

///mock storage
#[derive(Default, Debug)]
pub struct MockStorage {
    data: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MockStorage {
    pub fn new() -> Self {
        MockStorage::default()
    }
}

impl Storage for MockStorage {
    fn get(&self, key: &[u8]) -> FfiResult<Option<Vec<u8>>> {
        let gas_info = cosmwasm_vm::GasInfo::with_externally_used(key.len() as u64);
        (Ok(self.data.get(key).cloned()), gas_info)
    }

    #[cfg(feature = "iterator")]
    fn range<'a>(
        &'a self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        order: Order,
    ) -> FfiResult<Box<dyn Iterator<Item = FfiResult<KV>> + 'a>> {
        let bounds = range_bounds(start, end);

        // BTreeMap.range panics if range is start > end.
        // However, this cases represent just empty range and we treat it as such.
        match (bounds.start_bound(), bounds.end_bound()) {
            (Bound::Included(start), Bound::Excluded(end)) if start > end => {
                return Ok(Box::new(iter::empty()));
            }
            _ => {}
        }

        let iter = self.data.range(bounds);
        Ok(match order {
            Order::Ascending => Box::new(iter.map(clone_item).map(FfiResult::Ok)),
            Order::Descending => Box::new(iter.rev().map(clone_item).map(FfiResult::Ok)),
        })
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> FfiResult<()> {
        self.data.insert(key.to_vec(), value.to_vec());
        let gas_info = cosmwasm_vm::GasInfo::with_externally_used((key.len() + value.len()) as u64);
        watcher::logger_storage_event_insert(key, value);
        (Ok(()), gas_info)
    }

    fn remove(&mut self, key: &[u8]) -> FfiResult<()> {
        self.data.remove(key);
        let gas_info = cosmwasm_vm::GasInfo::with_externally_used(key.len() as u64);
        (Ok(()), gas_info)
    }
}

//mock api
#[derive(Copy, Clone)]
pub struct MockApi {
    canonical_length: usize,
}

impl MockApi {
    pub fn new(canonical_length: usize) -> Self {
        MockApi { canonical_length }
    }
}

impl Default for MockApi {
    fn default() -> Self {
        Self::new(20)
    }
}

impl Api for MockApi {
    fn canonical_address(&self, human: &HumanAddr) -> FfiResult<CanonicalAddr> {
        let gas_info = cosmwasm_vm::GasInfo::with_cost(GAS_COST_CANONICALIZE);
        // Dummy input validation. This is more sophisticated for formats like bech32, where format and checksum are validated.
        if human.len() < 3 {
            return (
                Err(FfiError::unknown("Invalid input: human address too short")),
                gas_info,
            );
        }
        if human.len() > self.canonical_length {
            return (
                Err(FfiError::unknown("Invalid input: human address too long")),
                gas_info,
            );
        }

        let mut out = Vec::from(human.as_str());
        let append = self.canonical_length - out.len();
        if append > 0 {
            out.extend(vec![0u8; append]);
        }
        (Ok(CanonicalAddr(Binary(out))), gas_info)
    }

    fn human_address(&self, canonical: &CanonicalAddr) -> FfiResult<HumanAddr> {
        let gas_info = cosmwasm_vm::GasInfo::with_cost(GAS_COST_HUMANIZE);

        if canonical.len() != self.canonical_length {
            return (
                Err(FfiError::unknown(
                    "Invalid input: canonical address length not correct",
                )),
                gas_info,
            );
        }

        let mut tmp: Vec<u8> = canonical.clone().into();
        // Shuffle two more times which restored the original value (24 elements are back to original after 20 rounds)
        for _ in 0..2 {
            tmp = cosmwasm_std::testing::riffle_shuffle(&tmp);
        }
        // Rotate back
        let rotate_by = cosmwasm_std::testing::digit_sum(&tmp) % self.canonical_length;
        tmp.rotate_right(rotate_by);
        // Remove NULL bytes (i.e. the padding)
        let trimmed = tmp.into_iter().filter(|&x| x != 0x00).collect();

        let result = match String::from_utf8(trimmed) {
            Ok(human) => Ok(HumanAddr(human)),
            Err(err) => Err(err.into()),
        };
        (result, gas_info)
    }
}

pub fn new_mock(
    canonical_length: usize,
    contract_balance: &[Coin],
    contract_addr: &str,
) -> Extern<MockStorage, MockApi, cosmwasm_vm::testing::MockQuerier> {
    let human_addr = HumanAddr::from(contract_addr);
    Extern {
        storage: MockStorage::default(),
        api: MockApi::new(canonical_length),
        querier: cosmwasm_vm::testing::MockQuerier::new(&[(&human_addr, contract_balance)]),
    }
}
