use cosmwasm_std::{
    attr, ensure, entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError, SubMsg,
};

use crate::state::{escrows, get_key, get_keys_for_recipient};
use ado_base::ADOContract;
use andromeda_finance::timelock::{
    Escrow, EscrowCondition, ExecuteMsg, GetLockedFundsForRecipientResponse,
    GetLockedFundsResponse, InstantiateMsg, MigrateMsg, QueryMsg,
};
use common::{
    ado_base::{hooks::AndromedaHook, recipient::Recipient, InstantiateMsg as BaseInstantiateMsg},
    encode_binary,
    error::ContractError,
};
use cw2::{get_contract_version, set_contract_version};
use semver::Version;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:andromeda-timelock";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    ADOContract::default().instantiate(
        deps.storage,
        env,
        deps.api,
        info,
        BaseInstantiateMsg {
            ado_type: "timelock".to_string(),
            ado_version: CONTRACT_VERSION.to_string(),
            operators: None,
            modules: msg.modules,
            primitive_contract: None,
        },
    )
}

#[entry_point]
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
            payload: encode_binary(&msg)?,
        },
    )?;

    match msg {
        ExecuteMsg::HoldFunds {
            condition,
            recipient,
        } => execute_hold_funds(deps, info, env, condition, recipient),
        ExecuteMsg::ReleaseFunds {
            recipient_addr,
            start_after,
            limit,
        } => execute_release_funds(deps, env, info, recipient_addr, start_after, limit),
        ExecuteMsg::ReleaseSpecificFunds {
            owner,
            recipient_addr,
        } => execute_release_specific_funds(deps, env, info, owner, recipient_addr),
        ExecuteMsg::AndrReceive(msg) => {
            ADOContract::default().execute(deps, env, info, msg, execute)
        }
    }
}

fn execute_hold_funds(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    condition: Option<EscrowCondition>,
    recipient: Option<Recipient>,
) -> Result<Response, ContractError> {
    let rec = recipient.unwrap_or_else(|| Recipient::Addr(info.sender.to_string()));

    //Validate recipient address
    let recipient_addr = rec.get_addr(
        deps.api,
        &deps.querier,
        ADOContract::default().get_app_contract(deps.storage)?,
    )?;
    deps.api.addr_validate(&recipient_addr)?;
    let key = get_key(info.sender.as_str(), &recipient_addr);
    let mut escrow = Escrow {
        coins: info.funds,
        condition,
        recipient: rec,
        recipient_addr,
    };
    // Add funds to existing escrow if it exists.
    let existing_escrow = escrows().may_load(deps.storage, key.to_vec())?;
    if let Some(existing_escrow) = existing_escrow {
        // Keep the original condition.
        escrow.condition = existing_escrow.condition;
        escrow.add_funds(existing_escrow.coins);
    } else {
        // Only want to validate if the escrow doesn't exist already. This is because it might be
        // unlocked at this point, which is fine if funds are being added to it.
        escrow.validate(deps.api, &env.block)?;
    }
    escrows().save(deps.storage, key.to_vec(), &escrow)?;

    Ok(Response::default().add_attributes(vec![
        attr("action", "hold_funds"),
        attr("sender", info.sender),
        attr("recipient", format!("{:?}", escrow.recipient)),
        attr("condition", format!("{:?}", escrow.condition)),
    ]))
}

fn execute_release_funds(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    recipient_addr: Option<String>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<Response, ContractError> {
    let recipient_addr = recipient_addr.unwrap_or_else(|| info.sender.to_string());

    let keys = get_keys_for_recipient(deps.storage, &recipient_addr, start_after, limit)?;

    ensure!(!keys.is_empty(), ContractError::NoLockedFunds {});

    let mut msgs: Vec<SubMsg> = vec![];
    for key in keys.iter() {
        let funds: Escrow = escrows().load(deps.storage, key.clone())?;
        if !funds.is_locked(&env.block)? {
            let msg = funds.recipient.generate_msg_native(
                deps.api,
                &deps.querier,
                ADOContract::default().get_app_contract(deps.storage)?,
                funds.coins,
            )?;
            msgs.push(msg);
            escrows().remove(deps.storage, key.clone())?;
        }
    }

    ensure!(!msgs.is_empty(), ContractError::FundsAreLocked {});

    Ok(Response::new().add_submessages(msgs).add_attributes(vec![
        attr("action", "release_funds"),
        attr("recipient_addr", recipient_addr),
    ]))
}

fn execute_release_specific_funds(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let recipient = recipient.unwrap_or_else(|| info.sender.to_string());
    let key = get_key(&owner, &recipient);
    let escrow = escrows().may_load(deps.storage, key.clone())?;
    match escrow {
        None => Err(ContractError::NoLockedFunds {}),
        Some(escrow) => {
            ensure!(
                !escrow.is_locked(&env.block)?,
                ContractError::FundsAreLocked {}
            );
            escrows().remove(deps.storage, key)?;
            let msg = escrow.recipient.generate_msg_native(
                deps.api,
                &deps.querier,
                ADOContract::default().get_app_contract(deps.storage)?,
                escrow.coins,
            )?;
            Ok(Response::new().add_submessage(msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", recipient),
            ]))
        }
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

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::GetLockedFunds { owner, recipient } => {
            encode_binary(&query_held_funds(deps, owner, recipient)?)
        }
        QueryMsg::GetLockedFundsForRecipient {
            recipient,
            start_after,
            limit,
        } => encode_binary(&query_funds_for_recipient(
            deps,
            recipient,
            start_after,
            limit,
        )?),
        QueryMsg::AndrQuery(msg) => ADOContract::default().query(deps, env, msg, query),
    }
}

fn query_funds_for_recipient(
    deps: Deps,
    recipient: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<GetLockedFundsForRecipientResponse, ContractError> {
    let keys = get_keys_for_recipient(deps.storage, &recipient, start_after, limit)?;
    let mut recipient_escrows: Vec<Escrow> = vec![];
    for key in keys.iter() {
        recipient_escrows.push(escrows().load(deps.storage, key.to_vec())?);
    }
    Ok(GetLockedFundsForRecipientResponse {
        funds: recipient_escrows,
    })
}

fn query_held_funds(
    deps: Deps,
    owner: String,
    recipient: String,
) -> Result<GetLockedFundsResponse, ContractError> {
    let hold_funds = escrows().may_load(deps.storage, get_key(&owner, &recipient))?;
    Ok(GetLockedFundsResponse { funds: hold_funds })
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::ado_base::AndromedaMsg;
    use cosmwasm_std::{
        coin, coins, from_binary,
        testing::{mock_dependencies, mock_env, mock_info},
        BankMsg, Coin, Timestamp,
    };
    use cw721::Expiration;

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = "owner";
        let info = mock_info(owner, &[]);
        let msg = InstantiateMsg { modules: None };
        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();

        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn test_execute_hold_funds() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let owner = "owner";
        let funds = vec![Coin::new(1000, "uusd")];
        let condition = EscrowCondition::Expiration(Expiration::AtHeight(1));
        let info = mock_info(owner, &funds);

        let msg = ExecuteMsg::HoldFunds {
            condition: Some(condition.clone()),
            recipient: None,
        };
        env.block.height = 0;

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        let expected = Response::default().add_attributes(vec![
            attr("action", "hold_funds"),
            attr("sender", info.sender.to_string()),
            attr(
                "recipient",
                format!("{:?}", Recipient::Addr(info.sender.to_string())),
            ),
            attr("condition", format!("{:?}", Some(condition.clone()))),
        ]);
        assert_eq!(expected, res);

        let query_msg = QueryMsg::GetLockedFunds {
            owner: owner.to_string(),
            recipient: owner.to_string(),
        };

        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let val: GetLockedFundsResponse = from_binary(&res).unwrap();
        let expected = Escrow {
            coins: funds,
            condition: Some(condition),
            recipient: Recipient::Addr(owner.to_string()),
            recipient_addr: owner.to_string(),
        };

        assert_eq!(val.funds.unwrap(), expected);
    }

    #[test]
    fn test_execute_hold_funds_escrow_updated() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();

        let owner = "owner";
        let info = mock_info(owner, &coins(100, "uusd"));

        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::Expiration(Expiration::AtHeight(10))),
            recipient: Some(Recipient::Addr("recipient".into())),
        };

        env.block.height = 0;

        let _res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::Expiration(Expiration::AtHeight(100))),
            recipient: Some(Recipient::Addr("recipient".into())),
        };

        env.block.height = 120;

        let info = mock_info(owner, &[coin(100, "uusd"), coin(100, "uluna")]);
        let _res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let query_msg = QueryMsg::GetLockedFunds {
            owner: owner.to_string(),
            recipient: "recipient".to_string(),
        };

        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let val: GetLockedFundsResponse = from_binary(&res).unwrap();
        let expected = Escrow {
            // Coins get merged.
            coins: vec![coin(200, "uusd"), coin(100, "uluna")],
            // Original expiration remains.
            condition: Some(EscrowCondition::Expiration(Expiration::AtHeight(10))),
            recipient: Recipient::Addr("recipient".to_string()),
            recipient_addr: "recipient".to_string(),
        };

        assert_eq!(val.funds.unwrap(), expected);
    }

    #[test]
    fn test_execute_release_funds_block_condition() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::Expiration(Expiration::AtHeight(1))),
            recipient: None,
        };
        env.block.height = 0;
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        env.block.height = 2;
        let msg = ExecuteMsg::ReleaseFunds {
            recipient_addr: None,
            start_after: None,
            limit: None,
        };
        let res = execute(deps.as_mut(), env, info.clone(), msg).unwrap();
        let bank_msg = BankMsg::Send {
            to_address: "owner".into(),
            amount: info.funds,
        };
        assert_eq!(
            Response::new().add_message(bank_msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", "owner"),
            ]),
            res
        );
    }

    #[test]
    fn test_execute_release_funds_no_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: None,
            recipient: None,
        };
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseFunds {
            recipient_addr: None,
            start_after: None,
            limit: None,
        };
        let res = execute(deps.as_mut(), env, info.clone(), msg).unwrap();
        let bank_msg = BankMsg::Send {
            to_address: "owner".into(),
            amount: info.funds,
        };
        assert_eq!(
            Response::new().add_message(bank_msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", "owner"),
            ]),
            res
        );
    }

    #[test]
    fn test_execute_release_multiple_escrows() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let recipient = Recipient::Addr("recipient".into());

        let msg = ExecuteMsg::HoldFunds {
            condition: None,
            recipient: Some(recipient),
        };
        let info = mock_info("sender1", &coins(100, "uusd"));
        let _res = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();

        let info = mock_info("sender2", &coins(200, "uusd"));
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseFunds {
            recipient_addr: Some("recipient".into()),
            start_after: None,
            limit: None,
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();

        let bank_msg1 = BankMsg::Send {
            to_address: "recipient".into(),
            amount: coins(100, "uusd"),
        };
        let bank_msg2 = BankMsg::Send {
            to_address: "recipient".into(),
            amount: coins(200, "uusd"),
        };
        assert_eq!(
            Response::new()
                .add_messages(vec![bank_msg1, bank_msg2])
                .add_attributes(vec![
                    attr("action", "release_funds"),
                    attr("recipient_addr", "recipient"),
                ]),
            res
        );
    }

    #[test]
    fn test_execute_release_funds_time_condition() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::Expiration(Expiration::AtTime(
                Timestamp::from_seconds(100),
            ))),
            recipient: None,
        };
        env.block.time = Timestamp::from_seconds(50);
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseFunds {
            recipient_addr: None,
            start_after: None,
            limit: None,
        };

        env.block.time = Timestamp::from_seconds(150);
        let res = execute(deps.as_mut(), env, info.clone(), msg).unwrap();
        let bank_msg = BankMsg::Send {
            to_address: "owner".into(),
            amount: info.funds,
        };
        assert_eq!(
            Response::new().add_message(bank_msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", "owner"),
            ]),
            res
        );
    }

    #[test]
    fn test_execute_release_funds_locked() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::Expiration(Expiration::AtTime(
                Timestamp::from_seconds(100),
            ))),
            recipient: None,
        };
        env.block.time = Timestamp::from_seconds(50);
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseFunds {
            recipient_addr: None,
            start_after: None,
            limit: None,
        };

        let res = execute(deps.as_mut(), env, info, msg);
        assert_eq!(ContractError::FundsAreLocked {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_release_funds_min_funds_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::MinimumFunds(vec![
                coin(200, "uusd"),
                coin(100, "uluna"),
            ])),
            recipient: None,
        };
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseFunds {
            recipient_addr: None,
            start_after: None,
            limit: None,
        };

        let res = execute(deps.as_mut(), env.clone(), info, msg);
        assert_eq!(ContractError::FundsAreLocked {}, res.unwrap_err());

        // Update the escrow with enough funds.
        let msg = ExecuteMsg::HoldFunds {
            condition: None,
            recipient: None,
        };
        let info = mock_info(owner, &[coin(110, "uusd"), coin(120, "uluna")]);
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Now try to release funds.
        let msg = ExecuteMsg::ReleaseFunds {
            recipient_addr: None,
            start_after: None,
            limit: None,
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();

        let bank_msg = BankMsg::Send {
            to_address: "owner".into(),
            amount: vec![coin(210, "uusd"), coin(120, "uluna")],
        };
        assert_eq!(
            Response::new().add_message(bank_msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", "owner"),
            ]),
            res
        );
    }

    #[test]
    fn test_execute_release_specific_funds_no_funds_locked() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[]);
        let msg = ExecuteMsg::ReleaseSpecificFunds {
            recipient_addr: None,
            owner: owner.into(),
        };
        let res = execute(deps.as_mut(), env, info, msg);
        assert_eq!(ContractError::NoLockedFunds {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_release_specific_funds_no_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: None,
            recipient: None,
        };
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseSpecificFunds {
            recipient_addr: None,
            owner: owner.into(),
        };
        let res = execute(deps.as_mut(), env, info.clone(), msg).unwrap();
        let bank_msg = BankMsg::Send {
            to_address: "owner".into(),
            amount: info.funds,
        };
        assert_eq!(
            Response::new().add_message(bank_msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", "owner"),
            ]),
            res
        );
    }

    #[test]
    fn test_execute_release_specific_funds_time_condition() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::Expiration(Expiration::AtTime(
                Timestamp::from_seconds(100),
            ))),
            recipient: None,
        };
        env.block.time = Timestamp::from_seconds(50);
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseSpecificFunds {
            recipient_addr: None,
            owner: owner.into(),
        };

        env.block.time = Timestamp::from_seconds(150);
        let res = execute(deps.as_mut(), env, info.clone(), msg).unwrap();
        let bank_msg = BankMsg::Send {
            to_address: "owner".into(),
            amount: info.funds,
        };
        assert_eq!(
            Response::new().add_message(bank_msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", "owner"),
            ]),
            res
        );
    }

    #[test]
    fn test_execute_release_specific_funds_min_funds_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = "owner";

        let info = mock_info(owner, &[coin(100, "uusd")]);
        let msg = ExecuteMsg::HoldFunds {
            condition: Some(EscrowCondition::MinimumFunds(vec![
                coin(200, "uusd"),
                coin(100, "uluna"),
            ])),
            recipient: None,
        };
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        let msg = ExecuteMsg::ReleaseSpecificFunds {
            recipient_addr: None,
            owner: owner.into(),
        };

        let res = execute(deps.as_mut(), env.clone(), info, msg);
        assert_eq!(ContractError::FundsAreLocked {}, res.unwrap_err());

        // Update the escrow with enough funds.
        let msg = ExecuteMsg::HoldFunds {
            condition: None,
            recipient: None,
        };
        let info = mock_info(owner, &[coin(110, "uusd"), coin(120, "uluna")]);
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Now try to release funds.
        let msg = ExecuteMsg::ReleaseSpecificFunds {
            recipient_addr: None,
            owner: owner.into(),
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();

        let bank_msg = BankMsg::Send {
            to_address: "owner".into(),
            amount: vec![coin(210, "uusd"), coin(120, "uluna")],
        };
        assert_eq!(
            Response::new().add_message(bank_msg).add_attributes(vec![
                attr("action", "release_funds"),
                attr("recipient_addr", "owner"),
            ]),
            res
        );
    }

    #[test]
    fn test_execute_receive() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = "owner";
        let funds = vec![Coin::new(1000, "uusd")];
        let info = mock_info(owner, &funds);

        let msg_struct = ExecuteMsg::HoldFunds {
            condition: None,
            recipient: None,
        };
        let msg_string = encode_binary(&msg_struct).unwrap();

        let msg = ExecuteMsg::AndrReceive(AndromedaMsg::Receive(Some(msg_string)));

        let received = execute(deps.as_mut(), env, info.clone(), msg).unwrap();
        let expected = Response::default().add_attributes(vec![
            attr("action", "hold_funds"),
            attr("sender", info.sender.to_string()),
            attr("recipient", "Addr(\"owner\")"),
            attr("condition", "None"),
        ]);

        assert_eq!(expected, received)
    }
}
