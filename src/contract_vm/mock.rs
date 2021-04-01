extern crate base64;

use crate::contract_vm::watcher;

use cosmwasm_std::testing::MockQuerierCustomHandlerResult;
use cosmwasm_std::{
    to_binary, Binary, CanonicalAddr, Coin, CustomQuery, HumanAddr, SystemError, SystemResult,
};
use cosmwasm_vm::testing::MockQuerier;
use cosmwasm_vm::{Api, Backend, BackendError, BackendResult, Storage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(feature = "iterator")]
use std::{
    iter,
    ops::{Bound, RangeBounds},
};

const GAS_COST_HUMANIZE: u64 = 44;
const GAS_COST_CANONICALIZE: u64 = 55;

///mock storage, and custom query for oracle application
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
    fn get(&self, key: &[u8]) -> BackendResult<Option<Vec<u8>>> {
        let gas_info = cosmwasm_vm::GasInfo::with_externally_used(key.len() as u64);
        (Ok(self.data.get(key).cloned()), gas_info)
    }

    #[cfg(feature = "iterator")]
    fn range<'a>(
        &'a self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        order: Order,
    ) -> BackendResult<Box<dyn Iterator<Item = BackendResult<KV>> + 'a>> {
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
            Order::Ascending => Box::new(iter.map(clone_item).map(BackendResult::Ok)),
            Order::Descending => Box::new(iter.rev().map(clone_item).map(BackendResult::Ok)),
        })
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> BackendResult<()> {
        self.data.insert(key.to_vec(), value.to_vec());
        let gas_info = cosmwasm_vm::GasInfo::with_externally_used((key.len() + value.len()) as u64);
        watcher::logger_storage_event_insert(key, value);
        (Ok(()), gas_info)
    }

    fn remove(&mut self, key: &[u8]) -> BackendResult<()> {
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
    fn canonical_address(&self, human: &HumanAddr) -> BackendResult<CanonicalAddr> {
        let gas_info = cosmwasm_vm::GasInfo::with_cost(GAS_COST_CANONICALIZE);
        // Dummy input validation. This is more sophisticated for formats like bech32, where format and checksum are validated.
        if human.len() < 3 {
            return (
                Err(BackendError::unknown(
                    "Invalid input: human address too short",
                )),
                gas_info,
            );
        }
        if human.len() > self.canonical_length {
            return (
                Err(BackendError::unknown(
                    "Invalid input: human address too long",
                )),
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

    fn human_address(&self, canonical: &CanonicalAddr) -> BackendResult<HumanAddr> {
        let gas_info = cosmwasm_vm::GasInfo::with_cost(GAS_COST_HUMANIZE);

        if canonical.len() != self.canonical_length {
            return (
                Err(BackendError::unknown(
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
/// An implementation of QueryRequest::Custom to show this works and can be extended in the contract
pub enum SpecialQuery {
    Fetch {
        url: String,
        method: Option<String>,
        body: Option<String>,
        authorization: Option<String>,
    },
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SpecialResponse {
    data: String,
}

fn fetch(
    url: &String,
    method: &Option<String>,
    body: &Option<String>,
    authorization: &Option<String>,
) -> MockQuerierCustomHandlerResult {
    let mut req = match method {
        Some(v) => ureq::request(v, url),
        None => ureq::get(url),
    };

    // borrow ref
    if authorization.is_some() {
        req = req.set("Authorization", authorization.as_ref().unwrap());
    }

    let resp = match body {
        Some(v) => req.send_string(v),
        None => req.call(),
    };

    match resp {
        Ok(response) => {
            // return contract result
            let input = response.into_string().unwrap_or_default();
            // smart contract use base64 to decode bytes into structure
            let result = base64::encode(input.as_bytes());
            SystemResult::Ok(to_binary(&result).into())
        }
        Err(err) => SystemResult::Err(SystemError::InvalidRequest {
            error: err.to_string(),
            request: Binary::from([]),
        }),
    }
}

impl CustomQuery for SpecialQuery {}

pub fn custom_query_execute(query: &SpecialQuery) -> MockQuerierCustomHandlerResult {
    match query {
        SpecialQuery::Fetch {
            url,
            method,
            body,
            authorization,
        } => fetch(url, method, body, authorization),
    }
}

pub fn new_mock(
    canonical_length: usize,
    contract_balance: &[Coin],
    contract_addr: &str,
) -> Backend<MockApi, MockStorage, MockQuerier<SpecialQuery>> {
    let human_addr = HumanAddr::from(contract_addr);
    // update custom_querier
    let mut custom_querier: MockQuerier<SpecialQuery> =
        MockQuerier::new(&[(&human_addr, contract_balance)]);
    custom_querier =
        custom_querier.with_custom_handler(|query| -> MockQuerierCustomHandlerResult {
            custom_query_execute(&query)
        });
    Backend {
        api: MockApi::new(canonical_length),
        storage: MockStorage::default(),
        querier: custom_querier,
    }
}
