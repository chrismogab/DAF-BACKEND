use common::{ado_base::ownership::ContractOwnerResponse, error::ContractError};
use cosmwasm_std::{
    to_binary, DepsMut, QuerierWrapper, QueryRequest, StdResult, Storage, WasmQuery,
};
use cw_storage_plus::Map;

// pub const CONFIG: Item<Config> = Item::new("config");
pub const SYM_ADDRESS: Map<String, String> = Map::new("address");
pub const CODE_ID: Map<&str, u64> = Map::new("code_id");

// #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
// pub struct Config {
//     pub token_code_id: u64,
//     pub receipt_code_id: u64,
//     pub address_list_code_id: u64,
// }
pub fn store_code_id(storage: &mut dyn Storage, code_id_key: &str, code_id: u64) -> StdResult<()> {
    CODE_ID.save(storage, code_id_key, &code_id)
}

pub fn read_code_id(storage: &dyn Storage, code_id_key: &str) -> StdResult<u64> {
    CODE_ID.load(storage, code_id_key)
}
//
// pub fn store_config(storage: &mut dyn Storage, config: &Config) -> StdResult<()> {
//     CONFIG.save(storage, config)
// }
//
// pub fn read_config(storage: &dyn Storage) -> StdResult<Config> {
//     CONFIG.load(storage)
// }

pub fn store_address(storage: &mut dyn Storage, symbol: String, address: &str) -> StdResult<()> {
    SYM_ADDRESS.save(storage, symbol, &address.to_string())
}

pub fn read_address(storage: &dyn Storage, symbol: String) -> StdResult<String> {
    SYM_ADDRESS.load(storage, symbol)
}

pub fn is_address_defined(storage: &dyn Storage, symbol: String) -> StdResult<bool> {
    match read_address(storage, symbol) {
        Ok(_addr) => Ok(true),
        _ => Ok(false),
    }
}

pub fn is_creator(deps: &DepsMut, symbol: String, address: String) -> Result<bool, ContractError> {
    let contract_address = read_address(deps.storage, symbol)?;
    let owner = query_ado_owner(deps.querier, contract_address)?;

    Ok(owner == address)
}

fn query_ado_owner(querier: QuerierWrapper, addr: String) -> Result<String, ContractError> {
    let res: ContractOwnerResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: addr,
        msg: to_binary(&"")?,
    }))?;

    Ok(res.owner)
}
