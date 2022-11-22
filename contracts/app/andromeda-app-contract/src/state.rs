use andromeda_app::app::{AppComponent, ComponentAddress};
use common::{
    ado_base::{AndromedaMsg, ExecuteMsg},
    error::ContractError,
};
use cosmwasm_std::{to_binary, Addr, Coin, CosmosMsg, Order, ReplyOn, Storage, SubMsg, WasmMsg};
use cw_storage_plus::{Bound, Item, Map};

/// Used to store the addresses of each ADO within the app
pub const ADO_ADDRESSES: Map<&str, Addr> = Map::new("ado_addresses");
/// Stores a record of the describing structs for each ADO
pub const ADO_DESCRIPTORS: Map<&str, AppComponent> = Map::new("ado_descriptors");
pub const ADO_IDX: Item<u64> = Item::new("ado_idx");
pub const APP_NAME: Item<String> = Item::new("app_name");

// DEV NOTE: Very similar to CW721 module instantiation, possibly merge both implementations?
pub fn add_app_component(
    storage: &mut dyn Storage,
    component: &AppComponent,
) -> Result<u64, ContractError> {
    let idx = ADO_IDX.may_load(storage)?.unwrap_or(1u64);
    let idx_str = idx.to_string();
    ADO_DESCRIPTORS.save(storage, &idx_str, component)?;
    ADO_IDX.save(storage, &(idx + 1))?;

    Ok(idx)
}

pub fn load_component_addresses(storage: &dyn Storage) -> Result<Vec<Addr>, ContractError> {
    let min = Some(Bound::inclusive("1"));
    let addresses: Vec<Addr> = ADO_ADDRESSES
        .range(storage, min, None, Order::Ascending)
        .flatten()
        .map(|(_vec, addr)| addr)
        .collect();

    Ok(addresses)
}

pub fn load_component_addresses_with_name(
    storage: &dyn Storage,
) -> Result<Vec<ComponentAddress>, ContractError> {
    let min = Some(Bound::inclusive("1"));
    let addresses: Vec<ComponentAddress> = ADO_ADDRESSES
        .range(storage, min, None, Order::Ascending)
        .flatten()
        .map(|(name, addr)| ComponentAddress {
            name,
            address: addr.to_string(),
        })
        .collect();

    Ok(addresses)
}

pub fn load_component_descriptors(
    storage: &dyn Storage,
) -> Result<Vec<AppComponent>, ContractError> {
    let min = Some(Bound::inclusive("1"));
    let descriptors: Vec<AppComponent> = ADO_DESCRIPTORS
        .range(storage, min, None, Order::Ascending)
        .flatten()
        .map(|(_vec, component)| component)
        .collect();

    Ok(descriptors)
}

pub fn generate_ownership_message(addr: Addr, owner: &str) -> Result<SubMsg, ContractError> {
    let msg = to_binary(&ExecuteMsg::AndrReceive(AndromedaMsg::UpdateOwner {
        address: owner.to_string(),
    }))?;
    Ok(SubMsg {
        id: 101,
        reply_on: ReplyOn::Error,
        msg: CosmosMsg::Wasm(WasmMsg::Execute {
            msg,
            funds: Vec::<Coin>::new(),
            contract_addr: addr.to_string(),
        }),
        gas_limit: None,
    })
}

pub fn generate_assign_app_message(addr: &Addr, app_addr: &str) -> Result<SubMsg, ContractError> {
    let msg = to_binary(&ExecuteMsg::AndrReceive(AndromedaMsg::UpdateAppContract {
        address: app_addr.to_string(),
    }))?;
    Ok(SubMsg {
        id: 103,
        reply_on: ReplyOn::Error,
        msg: CosmosMsg::Wasm(WasmMsg::Execute {
            msg,
            funds: Vec::<Coin>::new(),
            contract_addr: addr.to_string(),
        }),
        gas_limit: None,
    })
}
