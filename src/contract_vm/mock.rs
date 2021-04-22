extern crate base64;
use cosmwasm_std::testing::MockQuerierCustomHandlerResult;
use cosmwasm_std::{
    from_slice, to_binary, to_vec, Binary, Coin, ContractResult, CustomQuery, Empty, HumanAddr,
    Querier as StdQuerier, QuerierResult, QueryRequest, SystemError, SystemResult,
};
use cosmwasm_vm::testing::{MockApi, MockStorage};
use cosmwasm_vm::{Backend, BackendError, BackendResult, GasInfo, Querier};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::contract_vm::querier::{CustomHandler, StdMockQuerier, WasmHandler};

/// Implement MockQuerier

const GAS_COST_QUERY_FLAT: u64 = 100_000;
/// Gas per request byte
const GAS_COST_QUERY_REQUEST_MULTIPLIER: u64 = 0;
/// Gas per reponse byte
const GAS_COST_QUERY_RESPONSE_MULTIPLIER: u64 = 100;

/// MockQuerier holds an immutable table of bank balances
/// TODO: also allow querying contracts
pub struct MockQuerier<C: CustomQuery + DeserializeOwned = Empty> {
    querier: StdMockQuerier<C>,
}

impl<C: CustomQuery + DeserializeOwned> MockQuerier<C> {
    pub fn new(
        balances: &[(&HumanAddr, &[Coin])],
        custom_handler: CustomHandler<C>,
        wasm_handler: WasmHandler,
    ) -> Self {
        MockQuerier {
            querier: StdMockQuerier::new(balances, custom_handler, wasm_handler),
        }
    }

    // set a new balance for the given address and return the old balance
    pub fn update_balance<U: Into<HumanAddr>>(
        &mut self,
        addr: U,
        balance: Vec<Coin>,
    ) -> Option<Vec<Coin>> {
        self.querier.update_balance(addr, balance)
    }

    #[cfg(feature = "staking")]
    pub fn update_staking(
        &mut self,
        denom: &str,
        validators: &[cosmwasm_std::Validator],
        delegations: &[cosmwasm_std::FullDelegation],
    ) {
        self.querier.update_staking(denom, validators, delegations);
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
pub enum SpecialQuery {
    Fetch {
        url: String,
        method: Option<String>,
        body: Option<String>,
        headers: Option<Vec<String>>,
    },
}

fn fetch(
    url: &String,
    method: &Option<String>,
    body: &Option<String>,
    headers: &Option<Vec<String>>,
) -> MockQuerierCustomHandlerResult {
    let mut req = match method {
        Some(v) => ureq::request(v, url),
        None => ureq::get(url),
    };

    // borrow ref
    if headers.is_some() {
        for line in headers.as_ref().unwrap() {
            // if can parse Header
            if let Ok(header) = ureq::Header::from_str(line) {
                req = req.set(header.name(), header.value().unwrap_or_default());
            }
        }
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
            request: Binary::from(url.as_bytes()),
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
            headers,
        } => fetch(url, method, body, headers),
    }
}

pub fn new_mock(
    contract_balance: &[Coin],
    contract_addr: &str,
    wasm_handler: WasmHandler,
) -> Backend<MockApi, MockStorage, MockQuerier<SpecialQuery>> {
    let human_addr = HumanAddr::from(contract_addr);
    // update custom_querier
    let custom_querier: MockQuerier<SpecialQuery> = MockQuerier::new(
        &[(&human_addr, contract_balance)],
        Box::new(|query| -> MockQuerierCustomHandlerResult { custom_query_execute(&query) }),
        wasm_handler,
    );
    Backend {
        api: MockApi::default(),
        storage: MockStorage::default(),
        querier: custom_querier,
    }
}
