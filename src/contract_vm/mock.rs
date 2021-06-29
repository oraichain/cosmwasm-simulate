use cosmwasm_std::testing::MockQuerierCustomHandlerResult;
use std::convert::TryInto;
use std::ops::{Bound, RangeBounds};

use cosmwasm_std::{
    from_slice, to_binary, to_vec, Binary, Coin, ContractResult, CustomQuery, Empty,
    Querier as StdQuerier, QuerierResult, QueryRequest, SystemError, SystemResult,
};

use cosmwasm_std::{Order, Pair};

use cosmwasm_vm::testing::MockApi;
use cosmwasm_vm::{Backend, BackendError, BackendResult, GasInfo, Querier, Storage};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::contract_vm::querier::{CustomHandler, StdMockQuerier, WasmHandler};
use crate::contract_vm::watcher;

/// Implement MockQuerier

const GAS_COST_QUERY_FLAT: u64 = 100_000;
/// Gas per request byte
const GAS_COST_QUERY_REQUEST_MULTIPLIER: u64 = 0;
/// Gas per reponse byte
const GAS_COST_QUERY_RESPONSE_MULTIPLIER: u64 = 100;

const GAS_COST_LAST_ITERATION: u64 = 37;
const GAS_COST_RANGE: u64 = 11;

/// MockQuerier holds an immutable table of bank balances
/// TODO: also allow querying contracts
pub struct MockQuerier<C: CustomQuery + DeserializeOwned = Empty> {
    querier: StdMockQuerier<C>,
}

impl<C: CustomQuery + DeserializeOwned> MockQuerier<C> {
    pub fn new(
        balances: &[(&str, &[Coin])],
        custom_handler: CustomHandler<C>,
        wasm_handler: WasmHandler,
    ) -> Self {
        MockQuerier {
            querier: StdMockQuerier::new(balances, custom_handler, wasm_handler),
        }
    }

    // set a new balance for the given address and return the old balance
    pub fn update_balance<U: Into<String>>(
        &mut self,
        addr: U,
        balance: Vec<Coin>,
    ) -> Option<Vec<Coin>> {
        self.querier.update_balance(addr, balance)
    }

    pub fn with_custom_handler<CH: 'static>(mut self, handler: CH) -> Self
    where
        CH: Fn(&C) -> MockQuerierCustomHandlerResult,
    {
        self.querier = self.querier.with_custom_handler(handler);
        self
    }
}

impl<C: CustomQuery + DeserializeOwned> StdQuerier for StdMockQuerier<C> {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let request: QueryRequest<C> = match from_slice(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return SystemResult::Err(SystemError::InvalidRequest {
                    error: format!("Parsing query request: {}", e),
                    request: bin_request.into(),
                })
            }
        };
        self.handle_query(&request)
    }
}

impl<C: CustomQuery + DeserializeOwned> Querier for MockQuerier<C> {
    fn query_raw(
        &self,
        bin_request: &[u8],
        gas_limit: u64,
    ) -> BackendResult<SystemResult<ContractResult<Binary>>> {
        let response = self.querier.raw_query(bin_request);
        let gas_info = GasInfo::with_externally_used(
            GAS_COST_QUERY_FLAT
                + (GAS_COST_QUERY_REQUEST_MULTIPLIER * (bin_request.len() as u64))
                + (GAS_COST_QUERY_RESPONSE_MULTIPLIER
                    * (to_binary(&response).unwrap().len() as u64)),
        );

        // In a production implementation, this should stop the query execution in the middle of the computation.
        // Thus no query response is returned to the caller.
        if gas_info.externally_used > gas_limit {
            return (Err(BackendError::out_of_gas()), gas_info);
        }

        // We don't use FFI in the mock implementation, so BackendResult is always Ok() regardless of error on other levels
        (Ok(response), gas_info)
    }
}

impl MockQuerier {
    pub fn query<C: CustomQuery>(
        &self,
        request: &QueryRequest<C>,
        gas_limit: u64,
    ) -> BackendResult<SystemResult<ContractResult<Binary>>> {
        // encode the request, then call raw_query
        let request_binary = match to_vec(request) {
            Ok(raw) => raw,
            Err(err) => {
                let gas_info = GasInfo::with_externally_used(err.to_string().len() as u64);
                return (
                    Ok(SystemResult::Err(SystemError::InvalidRequest {
                        error: format!("Serializing query request: {}", err),
                        request: b"N/A".into(),
                    })),
                    gas_info,
                );
            }
        };
        self.query_raw(&request_binary, gas_limit)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
/// An implementation of QueryRequest::Custom to show this works and can be extended in the contract
pub enum SpecialQuery {}
impl CustomQuery for SpecialQuery {}

pub fn custom_query_execute(query: &SpecialQuery) -> MockQuerierCustomHandlerResult {
    SystemResult::Ok(to_binary(query).into())
}

#[derive(Default, Debug, Clone)]
pub struct Iter {
    data: Vec<Pair>,
    position: usize,
}

#[derive(Default, Debug, Clone)]
pub struct MockStorage {
    pub data: BTreeMap<Vec<u8>, Vec<u8>>,
    pub iterators: HashMap<u32, Iter>,
}

impl MockStorage {
    pub fn new() -> Self {
        MockStorage::default()
    }

    pub fn all(&mut self, iterator_id: u32) -> BackendResult<Vec<Pair>> {
        let mut out: Vec<Pair> = Vec::new();
        let mut total = GasInfo::free();
        loop {
            let (result, info) = self.next(iterator_id);
            total += info;
            match result {
                Err(err) => return (Err(err), total),
                Ok(ok) => {
                    if let Some(v) = ok {
                        out.push(v);
                    } else {
                        break;
                    }
                }
            }
        }
        (Ok(out), total)
    }
}

impl Storage for MockStorage {
    fn get(&self, key: &[u8]) -> BackendResult<Option<Vec<u8>>> {
        let gas_info = GasInfo::with_externally_used(key.len() as u64);
        (Ok(self.data.get(key).cloned()), gas_info)
    }

    fn scan(
        &mut self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        order: Order,
    ) -> BackendResult<u32> {
        let gas_info = GasInfo::with_externally_used(GAS_COST_RANGE);
        let bounds = range_bounds(start, end);

        let values: Vec<Pair> = match (bounds.start_bound(), bounds.end_bound()) {
            // BTreeMap.range panics if range is start > end.
            // However, this cases represent just empty range and we treat it as such.
            (Bound::Included(start), Bound::Excluded(end)) if start > end => Vec::new(),
            _ => match order {
                Order::Ascending => self.data.range(bounds).map(clone_item).collect(),
                Order::Descending => self.data.range(bounds).rev().map(clone_item).collect(),
            },
        };

        let last_id: u32 = self
            .iterators
            .len()
            .try_into()
            .expect("Found more iterator IDs than supported");
        let new_id = last_id + 1;
        let iter = Iter {
            data: values,
            position: 0,
        };
        self.iterators.insert(new_id, iter);

        (Ok(new_id), gas_info)
    }

    fn next(&mut self, iterator_id: u32) -> BackendResult<Option<Pair>> {
        let iterator = match self.iterators.get_mut(&iterator_id) {
            Some(i) => i,
            None => {
                return (
                    Err(BackendError::iterator_does_not_exist(iterator_id)),
                    GasInfo::free(),
                )
            }
        };

        let (value, gas_info): (Option<Pair>, GasInfo) = if iterator.data.len() > iterator.position
        {
            let item = iterator.data[iterator.position].clone();
            iterator.position += 1;
            let gas_cost = (item.0.len() + item.1.len()) as u64;
            (Some(item), GasInfo::with_cost(gas_cost))
        } else {
            (None, GasInfo::with_externally_used(GAS_COST_LAST_ITERATION))
        };

        (Ok(value), gas_info)
    }

    // watch changes
    fn set(&mut self, key: &[u8], value: &[u8]) -> BackendResult<()> {
        self.data.insert(key.to_vec(), value.to_vec());
        let gas_info = GasInfo::with_externally_used((key.len() + value.len()) as u64);
        watcher::logger_storage_event_insert(key, value);
        (Ok(()), gas_info)
    }

    fn remove(&mut self, key: &[u8]) -> BackendResult<()> {
        self.data.remove(key);
        let gas_info = GasInfo::with_externally_used(key.len() as u64);
        watcher::logger_storage_event_remove(key);
        (Ok(()), gas_info)
    }
}

fn range_bounds(start: Option<&[u8]>, end: Option<&[u8]>) -> impl RangeBounds<Vec<u8>> {
    (
        start.map_or(Bound::Unbounded, |x| Bound::Included(x.to_vec())),
        end.map_or(Bound::Unbounded, |x| Bound::Excluded(x.to_vec())),
    )
}

/// The BTreeMap specific key-value pair reference type, as returned by BTreeMap<Vec<u8>, T>::range.
/// This is internal as it can change any time if the map implementation is swapped out.
type BTreeMapPairRef<'a, T = Vec<u8>> = (&'a Vec<u8>, &'a T);

fn clone_item<T: Clone>(item_ref: BTreeMapPairRef<T>) -> Pair<T> {
    let (key, value) = item_ref;
    (key.clone(), value.clone())
}

pub fn new_mock(
    contract_balance: &[Coin],
    contract_addr: &str,
    wasm_handler: WasmHandler,
    storage: MockStorage,
) -> Backend<MockApi, MockStorage, MockQuerier<SpecialQuery>> {
    // update custom_querier
    let custom_querier: MockQuerier<SpecialQuery> = MockQuerier::new(
        &[(contract_addr, contract_balance)],
        Box::new(|query| -> MockQuerierCustomHandlerResult { custom_query_execute(&query) }),
        wasm_handler,
    );
    Backend {
        api: MockApi::default(),
        storage,
        querier: custom_querier,
    }
}
