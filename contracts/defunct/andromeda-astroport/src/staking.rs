use cosmwasm_std::{
    Addr, Api, CosmosMsg, DepsMut, Env, MessageInfo, QuerierWrapper, Response, Storage, Uint128,
    WasmMsg,
};

use crate::{
    primitive_keys::{ASTROPORT_ASTRO, ASTROPORT_GENERATOR, ASTROPORT_STAKING, ASTROPORT_XASTRO},
    querier::{query_amount_staked, query_pending_reward},
};
use ado_base::ADOContract;
use astroport::{
    generator::{Cw20HookMsg as GeneratorCw20HookMsg, ExecuteMsg as GeneratorExecuteMsg},
    staking::Cw20HookMsg as StakingCw20HookMsg,
};
use common::{encode_binary, error::ContractError, ensure!};
use cw20::Cw20ExecuteMsg;
use cw_asset::AssetInfo;
use std::cmp;

pub fn execute_stake_lp(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lp_token_contract: String,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let contract = ADOContract::default();
    let astroport_generator = contract.get_cached_address(deps.storage, ASTROPORT_GENERATOR)?;
    ensure!(
        contract.is_owner_or_operator(deps.storage, info.sender.as_str())?,
        ContractError::Unauthorized {}
    );
    let lp_token = AssetInfo::cw20(deps.api.addr_validate(&lp_token_contract)?);
    let balance = lp_token.query_balance(&deps.querier, env.contract.address)?;
    let amount = cmp::min(amount.unwrap_or(balance), balance);

    Ok(Response::new()
        .add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: lp_token_contract.clone(),
            msg: encode_binary(&Cw20ExecuteMsg::Send {
                contract: astroport_generator,
                amount,
                msg: encode_binary(&GeneratorCw20HookMsg::Deposit {})?,
            })?,
            funds: vec![],
        }))
        .add_attribute("action", "stake_lp")
        .add_attribute("amount", amount)
        .add_attribute("lp_token", lp_token_contract))
}

pub fn execute_unstake_lp(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lp_token_contract: String,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let contract = ADOContract::default();
    let astroport_generator = contract.get_cached_address(deps.storage, ASTROPORT_GENERATOR)?;
    ensure!(
        contract.is_owner_or_operator(deps.storage, info.sender.as_str())?,
        ContractError::Unauthorized {}
    );
    let lp_token = deps.api.addr_validate(&lp_token_contract)?;
    let amount_staked = query_amount_staked(
        &deps.querier,
        astroport_generator.clone(),
        lp_token.clone(),
        env.contract.address,
    )?;

    let amount = cmp::min(amount.unwrap_or(amount_staked), amount_staked);
    Ok(Response::new()
        .add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: astroport_generator,
            funds: vec![],
            msg: encode_binary(&GeneratorExecuteMsg::Withdraw { amount, lp_token })?,
        }))
        .add_attribute("action", "unstake_lp")
        .add_attribute("amount", amount)
        .add_attribute("lp_token", lp_token_contract))
}

pub fn execute_claim_lp_staking_rewards(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lp_token_contract: String,
    auto_stake: Option<bool>,
) -> Result<Response, ContractError> {
    let contract = ADOContract::default();
    let astroport_generator = contract.get_cached_address(deps.storage, ASTROPORT_GENERATOR)?;
    ensure!(
        contract.is_owner_or_operator(deps.storage, info.sender.as_str())?,
        ContractError::Unauthorized {}
    );
    let lp_token = deps.api.addr_validate(&lp_token_contract)?;
    let lp_unstake_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: astroport_generator.clone(),
        funds: vec![],
        msg: encode_binary(&GeneratorExecuteMsg::Withdraw {
            // Astroport auto-withdraws rewards when LP tokens are withdrawn, so we can initiate a withdraw
            // of 0 to get the rewards and leave the LP tokens there.
            amount: Uint128::zero(),
            lp_token: lp_token.clone(),
        })?,
    });
    let pending_reward = query_pending_reward(
        &deps.querier,
        astroport_generator,
        lp_token,
        env.contract.address,
    )?;
    let auto_stake = auto_stake.unwrap_or(false);
    let res = Response::new()
        .add_attribute("action", "claim_lp_staking_rewards")
        .add_message(lp_unstake_msg);
    if auto_stake {
        let stake_res = stake_or_unstake_astro(deps, info, pending_reward, true)?;
        Ok(res
            .add_attributes(stake_res.attributes)
            .add_submessages(stake_res.messages))
    } else {
        Ok(res)
    }
}

pub fn execute_stake_astro(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let balance = get_balance(
        deps.storage,
        deps.api,
        &deps.querier,
        env.contract.address,
        ASTROPORT_ASTRO,
    )?;
    let amount = cmp::min(amount.unwrap_or(balance), balance);
    stake_or_unstake_astro(deps, info, amount, true)
}

pub fn execute_unstake_astro(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let balance = get_balance(
        deps.storage,
        deps.api,
        &deps.querier,
        env.contract.address,
        ASTROPORT_XASTRO,
    )?;
    let amount = cmp::min(amount.unwrap_or(balance), balance);
    stake_or_unstake_astro(deps, info, amount, false)
}

fn stake_or_unstake_astro(
    deps: DepsMut,
    info: MessageInfo,
    amount: Uint128,
    stake: bool,
) -> Result<Response, ContractError> {
    let contract = ADOContract::default();
    let astroport_astro = contract.get_cached_address(deps.storage, ASTROPORT_ASTRO)?;
    let astroport_xastro = contract.get_cached_address(deps.storage, ASTROPORT_XASTRO)?;
    let astroport_staking = contract.get_cached_address(deps.storage, ASTROPORT_STAKING)?;
    ensure!(
        contract.is_owner_or_operator(deps.storage, info.sender.as_str())?,
        ContractError::Unauthorized {}
    );

    let (token_addr, msg, action) = if stake {
        (astroport_astro, StakingCw20HookMsg::Enter {}, "stake_astro")
    } else {
        (
            astroport_xastro,
            StakingCw20HookMsg::Leave {},
            "unstake_astro",
        )
    };

    Ok(Response::new()
        .add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: token_addr,
            msg: encode_binary(&Cw20ExecuteMsg::Send {
                contract: astroport_staking,
                amount,
                msg: encode_binary(&msg)?,
            })?,
            funds: vec![],
        }))
        .add_attribute("action", action)
        .add_attribute("amount", amount))
}

fn get_balance(
    storage: &dyn Storage,
    api: &dyn Api,
    querier: &QuerierWrapper,
    contract_address: Addr,
    primitive_key: &str,
) -> Result<Uint128, ContractError> {
    let contract = ADOContract::default();
    let token_addr = contract.get_cached_address(storage, primitive_key)?;
    let token = AssetInfo::cw20(api.addr_validate(&token_addr)?);
    let balance = token.query_balance(querier, contract_address)?;

    Ok(balance)
}
