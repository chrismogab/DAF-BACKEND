#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, ensure, has_coins, to_binary, Addr, Api, BankMsg, Binary, Coin, CosmosMsg, Deps, DepsMut,
    Empty, Env, MessageInfo, QuerierWrapper, Response, StdError, Storage, SubMsg, Uint128,
};

use crate::state::{is_archived, ANDR_MINTER, ARCHIVED, TRANSFER_AGREEMENTS};
use ado_base::state::ADOContract;
use andromeda_non_fungible_tokens::cw721::{
    ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, TokenExtension, TransferAgreement,
};
use cw2::{get_contract_version, set_contract_version};
use semver::Version;

use common::{
    ado_base::{
        hooks::{AndromedaHook, OnFundsTransferResponse},
        AndromedaMsg, InstantiateMsg as BaseInstantiateMsg,
    },
    encode_binary,
    error::ContractError,
    rates::get_tax_amount,
    Funds,
};
use cw721::ContractInfoResponse;
use cw721_base::{state::TokenInfo, Cw721Contract, MintMsg};

pub type AndrCW721Contract<'a> = Cw721Contract<'a, TokenExtension, Empty>;
const CONTRACT_NAME: &str = "crates.io:andromeda-cw721";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

struct ExecuteEnv<'a> {
    deps: DepsMut<'a>,
    env: Env,
    info: MessageInfo,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let contract_info = ContractInfoResponse {
        name: msg.name,
        symbol: msg.symbol,
    };
    // Do this directly instead of with cw721_contract.instantiate because we want to have minter
    // be an AndrAddress, which cannot be validated right away.
    AndrCW721Contract::default()
        .contract_info
        .save(deps.storage, &contract_info)?;

    ANDR_MINTER.save(deps.storage, &msg.minter)?;

    ADOContract::default().instantiate(
        deps.storage,
        env,
        deps.api,
        info,
        BaseInstantiateMsg {
            ado_type: "cw721".to_string(),
            ado_version: CONTRACT_VERSION.to_string(),
            operators: None,
            modules: msg.modules,
            primitive_contract: None,
        },
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let execute_env = ExecuteEnv { deps, env, info };
    let contract = ADOContract::default();

    // Do this before the hooks get fired off to ensure that there are no errors from the app
    // address not being fully setup yet.
    // Handled separately due to extra data required
    if let ExecuteMsg::AndrReceive(AndromedaMsg::UpdateAppContract { address }) = msg {
        let andr_minter = ANDR_MINTER.load(execute_env.deps.storage)?;
        return contract.execute_update_app_contract(
            execute_env.deps,
            execute_env.info,
            address,
            Some(vec![andr_minter]),
        );
    };

    //Andromeda Messages can be executed without modules, if they are a wrapped execute message they will loop back
    if let ExecuteMsg::AndrReceive(andr_msg) = msg {
        return contract.execute(
            execute_env.deps,
            execute_env.env,
            execute_env.info,
            andr_msg,
            execute,
        );
    };

    if let ExecuteMsg::Approve { token_id, .. } = &msg {
        ensure!(
            !is_archived(execute_env.deps.storage, token_id)?,
            ContractError::TokenIsArchived {}
        );
    }

    contract.module_hook::<Response>(
        execute_env.deps.storage,
        execute_env.deps.api,
        execute_env.deps.querier,
        AndromedaHook::OnExecute {
            sender: execute_env.info.sender.to_string(),
            payload: encode_binary(&msg)?,
        },
    )?;

    match msg {
        ExecuteMsg::Mint(_) => execute_mint(execute_env, msg),
        ExecuteMsg::BatchMint { tokens } => execute_batch_mint(execute_env, tokens),
        ExecuteMsg::TransferNft {
            recipient,
            token_id,
        } => execute_transfer(execute_env, recipient, token_id),
        ExecuteMsg::TransferAgreement {
            token_id,
            agreement,
        } => execute_update_transfer_agreement(execute_env, token_id, agreement),
        ExecuteMsg::Archive { token_id } => execute_archive(execute_env, token_id),
        ExecuteMsg::Burn { token_id } => execute_burn(execute_env, token_id),
        _ => Ok(AndrCW721Contract::default().execute(
            execute_env.deps,
            execute_env.env,
            execute_env.info,
            msg.into(),
        )?),
    }
}

fn execute_mint(env: ExecuteEnv, msg: ExecuteMsg) -> Result<Response, ContractError> {
    let ExecuteEnv { deps, info, env } = env;
    let cw721_contract = AndrCW721Contract::default();
    let app_contract = ADOContract::default().get_app_contract(deps.storage)?;
    let andr_minter = ANDR_MINTER.load(deps.storage)?;
    if cw721_contract.minter.may_load(deps.storage)?.is_none() {
        let addr = deps.api.addr_validate(&andr_minter.get_address(
            deps.api,
            &deps.querier,
            app_contract,
        )?)?;
        save_minter(&cw721_contract, deps.storage, &addr)?;
    }

    Ok(cw721_contract.execute(deps, env, info, msg.into())?)
}

fn execute_batch_mint(
    env: ExecuteEnv,
    tokens_to_mint: Vec<MintMsg<TokenExtension>>,
) -> Result<Response, ContractError> {
    let ExecuteEnv {
        mut deps,
        info,
        env,
    } = env;
    let mut resp = Response::default();
    let cw721_contract = AndrCW721Contract::default();
    let app_contract = ADOContract::default().get_app_contract(deps.storage)?;
    let andr_minter = ANDR_MINTER.load(deps.storage)?;
    if cw721_contract.minter.may_load(deps.storage)?.is_none() {
        let addr = deps.api.addr_validate(&andr_minter.get_address(
            deps.api,
            &deps.querier,
            app_contract,
        )?)?;
        save_minter(&cw721_contract, deps.storage, &addr)?;
    }
    for msg in tokens_to_mint {
        let mint_resp = cw721_contract.mint(deps.branch(), env.clone(), info.clone(), msg)?;
        resp = resp
            .add_attributes(mint_resp.attributes)
            .add_submessages(mint_resp.messages);
    }

    Ok(resp)
}

fn save_minter(
    cw721_contract: &AndrCW721Contract,
    storage: &mut dyn Storage,
    minter: &Addr,
) -> Result<(), ContractError> {
    Ok(cw721_contract.minter.save(storage, minter)?)
}

fn execute_transfer(
    env: ExecuteEnv,
    recipient: String,
    token_id: String,
) -> Result<Response, ContractError> {
    let ExecuteEnv { deps, info, env } = env;
    let base_contract = ADOContract::default();
    let responses = base_contract.module_hook::<Response>(
        deps.storage,
        deps.api,
        deps.querier,
        AndromedaHook::OnTransfer {
            token_id: token_id.clone(),
            sender: info.sender.to_string(),
            recipient: recipient.clone(),
        },
    )?;
    // Reduce all responses into one.
    let mut resp = responses
        .into_iter()
        .reduce(|resp, r| {
            resp.add_submessages(r.messages)
                .add_events(r.events)
                .add_attributes(r.attributes)
        })
        .unwrap_or_else(Response::new);

    let contract = AndrCW721Contract::default();
    let mut token = contract.tokens.load(deps.storage, &token_id)?;
    ensure!(
        !is_archived(deps.storage, &token_id)?,
        ContractError::TokenIsArchived {}
    );

    let tax_amount = if let Some(agreement) =
        &TRANSFER_AGREEMENTS.may_load(deps.storage, &token_id)?
    {
        let app_contract = base_contract.get_app_contract(deps.storage)?;
        let agreement_amount =
            get_transfer_agreement_amount(deps.api, &deps.querier, app_contract, agreement)?;
        let (mut msgs, events, remainder) = base_contract.on_funds_transfer(
            deps.storage,
            deps.api,
            &deps.querier,
            info.sender.to_string(),
            Funds::Native(agreement_amount.clone()),
            encode_binary(&ExecuteMsg::TransferNft {
                token_id: token_id.clone(),
                recipient: recipient.clone(),
            })?,
        )?;
        let remaining_amount = remainder.try_get_coin()?;
        let tax_amount = get_tax_amount(&msgs, agreement_amount.amount, remaining_amount.amount);
        msgs.push(SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
            to_address: token.owner.to_string(),
            amount: vec![remaining_amount],
        })));
        resp = resp.add_submessages(msgs).add_events(events);
        tax_amount
    } else {
        Uint128::zero()
    };

    ensure!(
        !is_archived(deps.storage, &token_id)?,
        ContractError::TokenIsArchived {}
    );
    check_can_send(deps.as_ref(), env, info, &token_id, &token, tax_amount)?;
    token.owner = deps.api.addr_validate(&recipient)?;
    token.approvals.clear();
    contract.tokens.save(deps.storage, &token_id, &token)?;
    Ok(resp
        .add_attribute("action", "transfer")
        .add_attribute("recipient", recipient))
}

fn get_transfer_agreement_amount(
    api: &dyn Api,
    querier: &QuerierWrapper,
    app_contract: Option<Addr>,
    agreement: &TransferAgreement,
) -> Result<Coin, ContractError> {
    let agreement_amount = agreement
        .amount
        .clone()
        .try_into_coin(api, querier, app_contract)?;
    match agreement_amount {
        Some(amount) => Ok(amount),
        None => Err(ContractError::PrimitiveDoesNotExist {
            msg: "TransferAgreement price is None".to_string(),
        }),
    }
}

fn check_can_send(
    deps: Deps,
    env: Env,
    info: MessageInfo,
    token_id: &str,
    token: &TokenInfo<TokenExtension>,
    tax_amount: Uint128,
) -> Result<(), ContractError> {
    // owner can send
    if token.owner == info.sender {
        return Ok(());
    }

    // token purchaser can send if correct funds are sent
    if let Some(agreement) = &TRANSFER_AGREEMENTS.may_load(deps.storage, token_id)? {
        let app_contract = ADOContract::default().get_app_contract(deps.storage)?;
        let agreement_amount =
            get_transfer_agreement_amount(deps.api, &deps.querier, app_contract, agreement)?;
        ensure!(
            has_coins(
                &info.funds,
                &Coin {
                    denom: agreement_amount.denom.to_owned(),
                    // Ensure that the taxes came from the sender.
                    amount: agreement_amount.amount + tax_amount,
                },
            ),
            ContractError::InsufficientFunds {}
        );
        if agreement.purchaser == info.sender || agreement.purchaser == "*" {
            return Ok(());
        }
    }

    // any non-expired token approval can send
    if token
        .approvals
        .iter()
        .any(|apr| apr.spender == info.sender && !apr.is_expired(&env.block))
    {
        return Ok(());
    }

    // operator can send
    let op = AndrCW721Contract::default()
        .operators
        .may_load(deps.storage, (&token.owner, &info.sender))?;
    match op {
        Some(ex) => {
            if ex.is_expired(&env.block) {
                Err(ContractError::Unauthorized {})
            } else {
                Ok(())
            }
        }
        None => Err(ContractError::Unauthorized {}),
    }
}

fn execute_update_transfer_agreement(
    env: ExecuteEnv,
    token_id: String,
    agreement: Option<TransferAgreement>,
) -> Result<Response, ContractError> {
    let ExecuteEnv { deps, info, .. } = env;
    let contract = AndrCW721Contract::default();
    let token = contract.tokens.load(deps.storage, &token_id)?;
    ensure!(token.owner == info.sender, ContractError::Unauthorized {});
    ensure!(
        !is_archived(deps.storage, &token_id)?,
        ContractError::TokenIsArchived {}
    );
    if let Some(xfer_agreement) = &agreement {
        TRANSFER_AGREEMENTS.save(deps.storage, &token_id, xfer_agreement)?;
        if xfer_agreement.purchaser != "*" {
            deps.api.addr_validate(&xfer_agreement.purchaser)?;
        }
    } else {
        TRANSFER_AGREEMENTS.remove(deps.storage, &token_id);
    }

    contract
        .tokens
        .save(deps.storage, token_id.as_str(), &token)?;

    Ok(Response::default())
}

fn execute_archive(env: ExecuteEnv, token_id: String) -> Result<Response, ContractError> {
    let ExecuteEnv { deps, info, .. } = env;
    ensure!(
        !is_archived(deps.storage, &token_id)?,
        ContractError::TokenIsArchived {}
    );
    let contract = AndrCW721Contract::default();
    let token = contract.tokens.load(deps.storage, &token_id)?;
    ensure!(token.owner == info.sender, ContractError::Unauthorized {});

    ARCHIVED.save(deps.storage, &token_id, &true)?;

    contract.tokens.save(deps.storage, &token_id, &token)?;

    Ok(Response::default())
}

fn execute_burn(env: ExecuteEnv, token_id: String) -> Result<Response, ContractError> {
    let ExecuteEnv { deps, info, .. } = env;
    let contract = AndrCW721Contract::default();
    let token = contract.tokens.load(deps.storage, &token_id)?;
    ensure!(token.owner == info.sender, ContractError::Unauthorized {});
    ensure!(
        !is_archived(deps.storage, &token_id)?,
        ContractError::TokenIsArchived {}
    );

    contract.tokens.remove(deps.storage, &token_id)?;

    // Decrement token count.
    let count = contract.token_count.load(deps.storage)?;
    contract.token_count.save(deps.storage, &(count - 1))?;

    Ok(Response::default().add_attributes(vec![
        attr("action", "burn"),
        attr("token_id", token_id),
        attr("sender", info.sender),
    ]))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::AndrHook(msg) => handle_andr_hook(deps, msg),
        QueryMsg::AndrQuery(msg) => ADOContract::default().query(deps, env, msg, query),
        QueryMsg::IsArchived { token_id } => Ok(to_binary(&is_archived(deps.storage, &token_id)?)?),
        QueryMsg::TransferAgreement { token_id } => {
            Ok(to_binary(&query_transfer_agreement(deps, token_id)?)?)
        }
        _ => Ok(AndrCW721Contract::default().query(deps, env, msg.into())?),
    }
}

pub fn query_transfer_agreement(
    deps: Deps,
    token_id: String,
) -> Result<Option<TransferAgreement>, ContractError> {
    Ok(TRANSFER_AGREEMENTS.may_load(deps.storage, &token_id)?)
}

fn handle_andr_hook(deps: Deps, msg: AndromedaHook) -> Result<Binary, ContractError> {
    match msg {
        AndromedaHook::OnFundsTransfer {
            sender,
            payload: _,
            amount,
        } => {
            let (msgs, events, remainder) = ADOContract::default().on_funds_transfer(
                deps.storage,
                deps.api,
                &deps.querier,
                sender,
                amount,
                encode_binary(&String::default())?,
            )?;
            let res = OnFundsTransferResponse {
                msgs,
                events,
                leftover_funds: remainder,
            };
            Ok(encode_binary(&Some(res))?)
        }
        _ => Ok(encode_binary(&None::<Response>)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    // New version
    let version: Version = CONTRACT_VERSION.parse().map_err(from_semver)?;

    // Old version
    let stored = get_contract_version(deps.storage)?;
    let storage_version: Version = stored.version.parse().map_err(from_semver)?;

    let contract = ADOContract::default();

    ensure!(
        stored.contract == CONTRACT_NAME,
        ContractError::CannotMigrate {
            previous_contract: stored.contract,
        }
    );

    // New version has to be newer/greater than the old version
    ensure!(
        storage_version < version,
        ContractError::CannotMigrate {
            previous_contract: stored.version,
        }
    );

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Update the ADOContract's version
    contract.execute_update_version(deps)?;

    Ok(Response::default())
}

fn from_semver(err: semver::Error) -> StdError {
    StdError::generic_err(format!("Semver: {}", err))
}
