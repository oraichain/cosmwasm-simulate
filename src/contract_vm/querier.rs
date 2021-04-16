use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use cosmwasm_std::testing::MockQuerierCustomHandlerResult;
use cosmwasm_std::{
    to_binary, AllBalanceResponse, AllDelegationsResponse, BalanceResponse, BankQuery, Binary,
    BondedDenomResponse, Coin, ContractResult, CustomQuery, Empty, FullDelegation, HumanAddr,
    QuerierResult, QueryRequest, StakingQuery, SystemError, SystemResult, Validator,
    ValidatorsResponse, WasmQuery,
};

/// DelegationResponse is data format returned from StakingRequest::Delegation query
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct DelegationResponse {
    pub delegation: Option<FullDelegation>,
}

/// StdMockQuerier holds an immutable table of bank balances
/// TODO: also allow querying contracts
pub struct StdMockQuerier<C: DeserializeOwned = Empty> {
    bank: BankQuerier,
    staking: StakingQuerier,
    // placeholder to add support later
    wasm: WasmQuerier,
    /// A handler to handle custom queries. This is set to a dummy handler that
    /// always errors by default. Update it via `with_custom_handler`.
    ///
    /// Use box to avoid the need of another generic type
    custom_handler: Box<dyn for<'a> Fn(&'a C) -> MockQuerierCustomHandlerResult>,
}

impl<C: DeserializeOwned> StdMockQuerier<C> {
    pub fn new(balances: &[(&HumanAddr, &[Coin])]) -> Self {
        StdMockQuerier {
            bank: BankQuerier::new(balances),
            staking: StakingQuerier::default(),
            wasm: WasmQuerier {},
            // strange argument notation suggested as a workaround here: https://github.com/rust-lang/rust/issues/41078#issuecomment-294296365
            custom_handler: Box::from(|_: &_| -> MockQuerierCustomHandlerResult {
                SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: "custom".to_string(),
                })
            }),
        }
    }

    // set a new balance for the given address and return the old balance
    pub fn update_balance<U: Into<HumanAddr>>(
        &mut self,
        addr: U,
        balance: Vec<Coin>,
    ) -> Option<Vec<Coin>> {
        self.bank.balances.insert(addr.into(), balance)
    }

    #[cfg(feature = "staking")]
    pub fn update_staking(
        &mut self,
        denom: &str,
        validators: &[crate::query::Validator],
        delegations: &[crate::query::FullDelegation],
    ) {
        self.staking = StakingQuerier::new(denom, validators, delegations);
    }

    pub fn with_custom_handler<CH: 'static>(mut self, handler: CH) -> Self
    where
        CH: Fn(&C) -> MockQuerierCustomHandlerResult,
    {
        self.custom_handler = Box::from(handler);
        self
    }
}

impl<C: CustomQuery + DeserializeOwned> StdMockQuerier<C> {
    pub fn handle_query(&self, request: &QueryRequest<C>) -> QuerierResult {
        match &request {
            QueryRequest::Bank(bank_query) => self.bank.query(bank_query),
            QueryRequest::Custom(custom_query) => (*self.custom_handler)(custom_query),
            QueryRequest::Staking(staking_query) => self.staking.query(staking_query),
            QueryRequest::Wasm(msg) => self.wasm.query(msg),
        }
    }
}

#[derive(Clone, Default)]
struct WasmQuerier {
    // FIXME: actually provide a way to call out
}

impl WasmQuerier {
    fn query(&self, request: &WasmQuery) -> QuerierResult {
        let addr = match request {
            WasmQuery::Smart { contract_addr, .. } => contract_addr,
            WasmQuery::Raw { contract_addr, .. } => contract_addr,
        }
        .clone();
        SystemResult::Err(SystemError::NoSuchContract { addr })
    }
}

#[derive(Clone, Default)]
pub struct BankQuerier {
    balances: HashMap<HumanAddr, Vec<Coin>>,
}

impl BankQuerier {
    pub fn new(balances: &[(&HumanAddr, &[Coin])]) -> Self {
        let mut map = HashMap::new();
        for (addr, coins) in balances.iter() {
            map.insert(HumanAddr::from(addr), coins.to_vec());
        }
        BankQuerier { balances: map }
    }

    pub fn query(&self, request: &BankQuery) -> QuerierResult {
        let contract_result: ContractResult<Binary> = match request {
            BankQuery::Balance { address, denom } => {
                // proper error on not found, serialize result on found
                let amount = self
                    .balances
                    .get(address)
                    .and_then(|v| v.iter().find(|c| &c.denom == denom).map(|c| c.amount))
                    .unwrap_or_default();
                let bank_res = BalanceResponse {
                    amount: Coin {
                        amount,
                        denom: denom.to_string(),
                    },
                };
                to_binary(&bank_res).into()
            }
            BankQuery::AllBalances { address } => {
                // proper error on not found, serialize result on found
                let bank_res = AllBalanceResponse {
                    amount: self.balances.get(address).cloned().unwrap_or_default(),
                };
                to_binary(&bank_res).into()
            }
        };
        // system result is always ok in the mock implementation
        SystemResult::Ok(contract_result)
    }
}

#[derive(Clone, Default)]
pub struct StakingQuerier {
    denom: String,
    validators: Vec<Validator>,
    delegations: Vec<FullDelegation>,
}

impl StakingQuerier {
    pub fn new(denom: &str, validators: &[Validator], delegations: &[FullDelegation]) -> Self {
        StakingQuerier {
            denom: denom.to_string(),
            validators: validators.to_vec(),
            delegations: delegations.to_vec(),
        }
    }

    pub fn query(&self, request: &StakingQuery) -> QuerierResult {
        let contract_result: ContractResult<Binary> = match request {
            StakingQuery::BondedDenom {} => {
                let res = BondedDenomResponse {
                    denom: self.denom.clone(),
                };
                to_binary(&res).into()
            }
            StakingQuery::Validators {} => {
                let res = ValidatorsResponse {
                    validators: self.validators.clone(),
                };
                to_binary(&res).into()
            }
            StakingQuery::AllDelegations { delegator } => {
                let delegations: Vec<_> = self
                    .delegations
                    .iter()
                    .filter(|d| &d.delegator == delegator)
                    .cloned()
                    .map(|d| d.into())
                    .collect();
                let res = AllDelegationsResponse { delegations };
                to_binary(&res).into()
            }
            StakingQuery::Delegation {
                delegator,
                validator,
            } => {
                let delegation = self
                    .delegations
                    .iter()
                    .find(|d| &d.delegator == delegator && &d.validator == validator);
                let res = DelegationResponse {
                    delegation: delegation.cloned(),
                };
                to_binary(&res).into()
            }
        };
        // system result is always ok in the mock implementation
        SystemResult::Ok(contract_result)
    }
}
