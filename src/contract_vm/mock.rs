extern crate base64;

use cosmwasm_std::testing::MockQuerierCustomHandlerResult;
use cosmwasm_std::{to_binary, Binary, Coin, CustomQuery, HumanAddr, SystemError, SystemResult};
use cosmwasm_vm::testing::{MockApi, MockQuerier, MockStorage};
use cosmwasm_vm::Backend;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SpecialResponse {
    data: String,
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
                req = req.set(header.name(), header.value());
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
            headers,
        } => fetch(url, method, body, headers),
    }
}

pub fn new_mock(
    contract_balance: &[Coin],
    contract_addr: &str,
) -> Backend<MockApi, MockStorage, MockQuerier<SpecialQuery>> {
    let human_addr = HumanAddr::from(contract_addr);
    // update custom_querier
    let custom_querier: MockQuerier<SpecialQuery> =
        MockQuerier::new(&[(&human_addr, contract_balance)]).with_custom_handler(
            |query| -> MockQuerierCustomHandlerResult { custom_query_execute(&query) },
        );
    Backend {
        api: MockApi::default(),
        storage: MockStorage::default(),
        querier: custom_querier,
    }
}
