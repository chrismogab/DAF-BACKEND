#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    ensure, from_binary, to_binary, Addr, Api, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg, attr,
};


use ado_base::ADOContract;
use andromeda_fungible_tokens::{cw20::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, CW20HookMsg, GetTokenPriceResponse} };
use common::{
    ado_base::{hooks::AndromedaHook, InstantiateMsg as BaseInstantiateMsg},
    error::ContractError,
    Funds, encode_binary,
};
use cw2::{get_contract_version, set_contract_version};
use cw20::{Cw20Coin, Cw20ExecuteMsg, Cw20ReceiveMsg};
use crate::state::{TOKEN_PRICE, USDC_CONTRACT,BALANCES, ALLOWANCES, GET_TOKEN_INFO, TokenPrice, GetTokenInfo,  };
use cw20_base::{
    contract::{execute as execute_cw20, instantiate as cw20_instantiate, query as query_cw20},
    allowances::deduct_allowance
};
use semver::Version;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:andromeda-cw20";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(  
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let contract = ADOContract::default();
    let resp = contract.instantiate(
        deps.storage,
        env.clone(),
        deps.api,
        info.clone(),
        BaseInstantiateMsg {
            ado_type: "cw20".to_string(),
            ado_version: CONTRACT_VERSION.to_string(),
            operators: None,
            modules: msg.modules.clone(),
            primitive_contract: None,
        },
    )?;
    let token_price = TokenPrice { price: msg.token_price };
    TOKEN_PRICE.save(deps.storage, &token_price)?;
    let token_info = GetTokenInfo {owner:msg.owner.clone(), name: msg.name.clone(), symbol: msg.symbol.clone()};
    GET_TOKEN_INFO.save(deps.storage, &token_info)?;
    USDC_CONTRACT.save(deps.storage,&msg.usdc_address).unwrap();

    let cw20_resp = cw20_instantiate(deps, env, info, msg.into())?;

    
    Ok(resp
        .add_submessages(cw20_resp.messages)
        .add_attributes(cw20_resp.attributes))


}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let contract = ADOContract::default();

    //Andromeda Messages can be executed without modules, if they are a wrapped execute message they will loop back
    if let ExecuteMsg::AndrReceive(andr_msg) = msg {
        return contract.execute(deps, env, info, andr_msg, execute);
    };

    contract.module_hook::<Response>(
        deps.storage,
        deps.api, 
        deps.querier,
        AndromedaHook::OnExecute {
            sender: info.sender.to_string(),
            payload: to_binary(&msg)?,
        },
    )?;

    match msg {
        ExecuteMsg::IncreaseAllowancePurchase { spender, amount, owner } => execute_increase_allowance_purchase(deps, env, info, owner, spender, amount),
        ExecuteMsg::UpdatePrice { new_price } =>  execute_update_new_price(deps, info, new_price),
        ExecuteMsg::Transfer { recipient, amount } => {
            execute_transfer(deps, env, info, recipient, amount) 
        },
        ExecuteMsg::Receive(msg) => execute_receive_cw20(deps, env, info, msg),
        ExecuteMsg::Send {
            contract,
            amount,
            msg,
        } => execute_send(deps, env, info, contract, amount, msg),
        ExecuteMsg::AndrReceive(msg) => contract.execute(deps, env, info, msg, execute),
        _ => Ok(execute_cw20(deps, env, info, msg.into())?),
    }

}


fn execute_update_new_price(
    deps: DepsMut,
    info: MessageInfo,
    new_price: Uint128,
) -> Result<Response, ContractError> {

    ensure!(
        ADOContract::default().is_contract_owner(deps.storage, info.sender.as_str())?,
        ContractError::Unauthorized {}
    );
    let mut update_price = TOKEN_PRICE.load(deps.storage)?;
    update_price.price = new_price;
    TOKEN_PRICE.save(deps.storage, &update_price)?;

    Ok(Response::new().add_attributes(vec![attr("action", "update_prices")]))

}

fn execute_transfer(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let (msgs, events, remainder) = ADOContract::default().on_funds_transfer(
        deps.storage,
        deps.api,
        &deps.querier,
        info.sender.to_string(),
        Funds::Cw20(Cw20Coin {
            address: env.contract.address.to_string(),
            amount,
        }),
        to_binary(&ExecuteMsg::Transfer {
            amount,
            recipient: recipient.clone(),
        })?,
    )?;


    let remaining_amount = match remainder {
        Funds::Native(..) => amount, //What do we do in the case that the rates returns remaining amount as native funds?
        Funds::Cw20(coin) => coin.amount,
    };

    let mut resp = filter_out_cw20_messages(msgs, deps.storage, deps.api, &info.sender)?;

    // Continue with standard cw20 operation
    let cw20_resp = execute_cw20(
        deps,
        env,
        info,
        Cw20ExecuteMsg::Transfer {
            recipient,
            amount: remaining_amount,
        },
    )?;
    resp = resp.add_attributes(cw20_resp.attributes).add_events(events);
    Ok(resp)
}

fn execute_receive_cw20(
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError>  {
    let usdc_contract = USDC_CONTRACT.load(deps.storage)?;

    // Ensure that the only CW20 that can interact with the contract is USDC which is set at instantiation.
    if info.sender.to_string() != usdc_contract {
        return Err(ContractError::Unauthorized {});
      }

    let price = TOKEN_PRICE.load(deps.storage)?;
    let amount_to_transfer = msg.amount.checked_div(price.price).unwrap() ;
    let owner = GET_TOKEN_INFO.load(deps.storage)?;
    match from_binary(&msg.msg)? {
        CW20HookMsg::Allowancehook {} => execute_increase_allowance_purchase(deps, env, info, owner.owner,msg.sender, amount_to_transfer),
        CW20HookMsg::Transfer {} => execute_transfer_from_purchase(deps, env, info, owner.owner , msg.sender, amount_to_transfer)    

        }
    }

    pub fn execute_transfer_from_purchase(
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        owner: String,
        recipient: String,
        amount: Uint128,
    ) -> Result<Response, ContractError> {

        let rcpt_addr = deps.api.addr_validate(&recipient)?;
        let owner_addr = deps.api.addr_validate(&owner)?;
    
        deduct_allowance(deps.storage, &owner_addr, &rcpt_addr, &env.block, amount)?;
     
        BALANCES.update(
            deps.storage,
            &owner_addr,
            |balance: Option<Uint128>| -> StdResult<_> {
                Ok(balance.unwrap_or_default().checked_sub(amount)?)
            },
        )?;
        BALANCES.update(
            deps.storage,
            &rcpt_addr,
            |balance: Option<Uint128>| -> StdResult<_> { Ok(balance.unwrap_or_default() + amount) },
        )?;
    
        let res = Response::new().add_attributes(vec![
            attr("action", "transfer_from"),
            attr("from", owner),
            attr("to", recipient),
            attr("by", info.sender),
            attr("amount", amount),
        ]);
        Ok(res)
    }


 pub fn execute_increase_allowance_purchase(
        deps: DepsMut,
        _env: Env,
        info: MessageInfo,
        owner: String,
        spender: String,
        amount: Uint128,
    ) -> Result<Response, ContractError> {
        
        
        let spender_addr = deps.api.addr_validate(&spender)?;
        let owner_addr = deps.api.addr_validate(&owner)?;

        if spender_addr == info.sender {
            return Err(ContractError::CannotSetOwnAccount {});
        }
    
        ALLOWANCES.update(
            deps.storage,
            (&owner_addr, &spender_addr),
            |allow| -> StdResult<_> {
                let mut val = allow.unwrap_or_default();
                val.allowance += amount;
                Ok(val)
            },
        )?;
    
        let res = Response::new().add_attributes(vec![
            attr("action", "increase_allowance"),
            attr("owner", info.sender),
            attr("spender", spender),
            attr("amount", amount),
        ]);
        Ok(res)
    }

fn transfer_tokens(
    storage: &mut dyn Storage,
    sender: &Addr,
    recipient: &Addr,
    amount: Uint128,
) -> Result<(), ContractError> {
    BALANCES.update(
        storage,
        sender,
        |balance: Option<Uint128>| -> StdResult<_> {
            Ok(balance.unwrap_or_default().checked_sub(amount)?)
        },
    )?;
    BALANCES.update(
        storage,
        recipient,
        |balance: Option<Uint128>| -> StdResult<_> { Ok(balance.unwrap_or_default() + amount) },
    )?;
    Ok(())
}


fn execute_send(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    contract: String,
    amount: Uint128,
    msg: Binary,
) -> Result<Response, ContractError> {
    let (msgs, events, remainder) = ADOContract::default().on_funds_transfer(
        deps.storage,
        deps.api,
        &deps.querier,
        info.sender.to_string(),
        Funds::Cw20(Cw20Coin {
            address: env.contract.address.to_string(),
            amount,
        }),
        to_binary(&ExecuteMsg::Send {
            amount,
            contract: contract.clone(),
            msg: msg.clone(),
        })?,
    )?;

    let remaining_amount = match remainder {
        Funds::Native(..) => amount, //What do we do in the case that the rates returns remaining amount as native funds?
        Funds::Cw20(coin) => coin.amount,
    };

    let mut resp = filter_out_cw20_messages(msgs, deps.storage, deps.api, &info.sender)?;

    let cw20_resp = execute_cw20(
        deps,
        env,
        info,
        Cw20ExecuteMsg::Send {
            contract,
            amount: remaining_amount,
            msg,
        },
    )?;
    resp = resp
        .add_attributes(cw20_resp.attributes)
        .add_events(events)
        .add_submessages(cw20_resp.messages);

    Ok(resp)
}

fn filter_out_cw20_messages(
    msgs: Vec<SubMsg>,
    storage: &mut dyn Storage,
    api: &dyn Api,
    sender: &Addr,
) -> Result<Response, ContractError> {
    let mut resp: Response = Response::new();
    // Filter through payment messages to extract cw20 transfer messages to avoid looping
    for sub_msg in msgs {
        // Transfer messages are CosmosMsg::Wasm type
        if let CosmosMsg::Wasm(WasmMsg::Execute { msg: exec_msg, .. }) = sub_msg.msg.clone() {
            // If binary deserializes to a Cw20ExecuteMsg check the message type
            if let Ok(Cw20ExecuteMsg::Transfer { recipient, amount }) =
                from_binary::<Cw20ExecuteMsg>(&exec_msg) 
            {
                transfer_tokens(storage, sender, &api.addr_validate(&recipient)?, amount)?;
            } else {
                resp = resp.add_submessage(sub_msg);
            }
        } else {
            resp = resp.add_submessage(sub_msg);
        }
    }
    Ok(resp)
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

#[cfg_attr(not(feature = "library"), entry_point)]

pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::GetTokenPrice {} => encode_binary(&query_token_price(deps)?),
        QueryMsg::AndrQuery(msg) => ADOContract::default().query(deps, env, msg, query),
        _ => Ok(query_cw20(deps, env, msg.into())?),

       
   
    }
}

fn query_token_price(deps: Deps) -> Result<GetTokenPriceResponse, ContractError> {
    let config = TOKEN_PRICE.load(deps.storage)?;
    let get_price  = config.price;

    Ok(GetTokenPriceResponse { price: get_price } )
}





