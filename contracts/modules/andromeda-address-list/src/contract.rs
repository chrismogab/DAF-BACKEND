#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, ensure, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError,
};
use cw2::{get_contract_version, set_contract_version};

use crate::state::{add_address, includes_address, remove_address, IS_INCLUSIVE};
use ado_base::ADOContract;
use andromeda_modules::address_list::{
    ExecuteMsg, IncludesAddressResponse, InstantiateMsg, MigrateMsg, QueryMsg,
};
use common::{
    ado_base::{hooks::AndromedaHook, AndromedaQuery, InstantiateMsg as BaseInstantiateMsg},
    encode_binary,
    error::ContractError,
    parse_message,
};
use cw_utils::nonpayable;
use semver::Version;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:andromeda-addresslist";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    IS_INCLUSIVE.save(deps.storage, &msg.is_inclusive)?;
    ADOContract::default().instantiate(
        deps.storage,
        env,
        deps.api,
        info,
        BaseInstantiateMsg {
            ado_type: "address-list".to_string(),
            ado_version: CONTRACT_VERSION.to_string(),
            operators: None,
            modules: None,
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
    match msg {
        ExecuteMsg::AndrReceive(msg) => {
            ADOContract::default().execute(deps, env, info, msg, execute)
        }
        ExecuteMsg::AddAddress { address } => execute_add_address(deps, info, address),
        ExecuteMsg::RemoveAddress { address } => execute_remove_address(deps, info, address),
    }
}

fn execute_add_address(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    nonpayable(&info)?;
    ensure!(
        ADOContract::default().is_owner_or_operator(deps.storage, info.sender.as_str())?,
        ContractError::Unauthorized {}
    );
    add_address(deps.storage, &address)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "add_address"),
        attr("address", address),
    ]))
}

fn execute_remove_address(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    nonpayable(&info)?;

    ensure!(
        ADOContract::default().is_owner_or_operator(deps.storage, info.sender.as_str())?,
        ContractError::Unauthorized {}
    );

    remove_address(deps.storage, &address);

    Ok(Response::new().add_attributes(vec![
        attr("action", "remove_address"),
        attr("address", address),
    ]))
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
        QueryMsg::IncludesAddress { address } => encode_binary(&query_address(deps, &address)?),
        QueryMsg::AndrHook(msg) => handle_andr_hook(deps, msg),
        QueryMsg::AndrQuery(msg) => handle_andromeda_query(deps, env, msg),
    }
}

fn handle_andr_hook(deps: Deps, msg: AndromedaHook) -> Result<Binary, ContractError> {
    match msg {
        AndromedaHook::OnExecute { sender, .. } => {
            let is_included = includes_address(deps.storage, &sender)?;
            let is_inclusive = IS_INCLUSIVE.load(deps.storage)?;
            if is_included != is_inclusive {
                Err(ContractError::Unauthorized {})
            } else {
                Ok(to_binary(&None::<Response>)?)
            }
        }
        _ => Ok(to_binary(&None::<Response>)?),
    }
}

fn handle_andromeda_query(
    deps: Deps,
    env: Env,
    msg: AndromedaQuery,
) -> Result<Binary, ContractError> {
    match msg {
        AndromedaQuery::Get(data) => {
            let address: String = parse_message(&data)?;
            encode_binary(&query_address(deps, &address)?)
        }
        _ => ADOContract::default().query(deps, env, msg, query),
    }
}

fn query_address(deps: Deps, address: &str) -> Result<IncludesAddressResponse, ContractError> {
    Ok(IncludesAddressResponse {
        included: includes_address(deps.storage, address)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ADDRESS_LIST;
    use cosmwasm_std::from_binary;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};

    fn init(deps: DepsMut, info: MessageInfo) {
        instantiate(
            deps,
            mock_env(),
            info,
            InstantiateMsg { is_inclusive: true },
        )
        .unwrap();
    }

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let msg = InstantiateMsg { is_inclusive: true };
        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn test_add_address() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let operator = "creator";
        let info = mock_info(operator, &[]);

        let address = "whitelistee";

        init(deps.as_mut(), info.clone());

        ADOContract::default()
            .execute_update_operators(deps.as_mut(), info.clone(), vec![operator.to_owned()])
            .unwrap();

        let msg = ExecuteMsg::AddAddress {
            address: address.to_string(),
        };

        //add address for registered operator

        let res = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();
        let expected = Response::default().add_attributes(vec![
            attr("action", "add_address"),
            attr("address", address),
        ]);
        assert_eq!(expected, res);

        let whitelisted = ADDRESS_LIST.load(deps.as_ref().storage, address).unwrap();
        assert!(whitelisted);

        let included = ADDRESS_LIST.load(deps.as_ref().storage, "111").unwrap_err();

        match included {
            cosmwasm_std::StdError::NotFound { .. } => {}
            _ => {
                panic!();
            }
        }

        //add address for unregistered operator
        let unauth_info = mock_info("anyone", &[]);
        let res = execute(deps.as_mut(), env, unauth_info, msg).unwrap_err();
        assert_eq!(ContractError::Unauthorized {}, res);
    }

    #[test]
    fn test_remove_address() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let operator = "creator";
        let info = mock_info(operator, &[]);

        let address = "whitelistee";

        init(deps.as_mut(), info.clone());

        //save operator
        ADOContract::default()
            .execute_update_operators(deps.as_mut(), info.clone(), vec![operator.to_owned()])
            .unwrap();

        let msg = ExecuteMsg::RemoveAddress {
            address: address.to_string(),
        };

        //add address for registered operator
        let res = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();
        let expected = Response::default().add_attributes(vec![
            attr("action", "remove_address"),
            attr("address", address),
        ]);
        assert_eq!(expected, res);

        let included_is_err = ADDRESS_LIST.load(deps.as_ref().storage, address).is_err();
        assert!(included_is_err);

        //add address for unregistered operator
        let unauth_info = mock_info("anyone", &[]);
        let res = execute(deps.as_mut(), env, unauth_info, msg).unwrap_err();
        assert_eq!(ContractError::Unauthorized {}, res);
    }

    #[test]
    fn test_execute_hook_whitelist() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let operator = "creator";
        let info = mock_info(operator, &[]);

        let address = "whitelistee";

        // Mark it as a whitelist.
        IS_INCLUSIVE.save(deps.as_mut().storage, &true).unwrap();
        init(deps.as_mut(), info.clone());

        let msg = ExecuteMsg::AddAddress {
            address: address.to_string(),
        };
        let _res = execute(deps.as_mut(), env, info, msg).unwrap();

        let msg = QueryMsg::AndrHook(AndromedaHook::OnExecute {
            sender: address.to_string(),
            payload: encode_binary(&"".to_string()).unwrap(),
        });

        let res: Option<Response> =
            from_binary(&query(deps.as_ref(), mock_env(), msg).unwrap()).unwrap();
        assert_eq!(None, res);

        let msg = QueryMsg::AndrHook(AndromedaHook::OnExecute {
            sender: "random".to_string(),
            payload: encode_binary(&"".to_string()).unwrap(),
        });

        let res_err: ContractError = query(deps.as_ref(), mock_env(), msg).unwrap_err();
        assert_eq!(ContractError::Unauthorized {}, res_err);
    }

    #[test]
    fn test_execute_hook_blacklist() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let operator = "creator";
        let info = mock_info(operator, &[]);

        let address = "blacklistee";
        init(deps.as_mut(), info.clone());

        // Mark it as a blacklist.
        IS_INCLUSIVE.save(deps.as_mut().storage, &false).unwrap();
        ADOContract::default()
            .execute_update_operators(deps.as_mut(), info.clone(), vec![operator.to_owned()])
            .unwrap();

        let msg = ExecuteMsg::AddAddress {
            address: address.to_string(),
        };
        let _res = execute(deps.as_mut(), env, info, msg).unwrap();

        let msg = QueryMsg::AndrHook(AndromedaHook::OnExecute {
            sender: "random".to_string(),
            payload: encode_binary(&"".to_string()).unwrap(),
        });

        let res: Option<Response> =
            from_binary(&query(deps.as_ref(), mock_env(), msg).unwrap()).unwrap();
        assert_eq!(None, res);

        let msg = QueryMsg::AndrHook(AndromedaHook::OnExecute {
            sender: address.to_string(),
            payload: encode_binary(&"".to_string()).unwrap(),
        });

        let res_err: ContractError = query(deps.as_ref(), mock_env(), msg).unwrap_err();
        assert_eq!(ContractError::Unauthorized {}, res_err);
    }

    #[test]
    fn test_andr_get_query() {
        let mut deps = mock_dependencies();

        let address = "whitelistee";

        ADDRESS_LIST
            .save(deps.as_mut().storage, address, &true)
            .unwrap();

        let msg = QueryMsg::AndrQuery(AndromedaQuery::Get(Some(encode_binary(&address).unwrap())));

        let res: IncludesAddressResponse =
            from_binary(&query(deps.as_ref(), mock_env(), msg).unwrap()).unwrap();

        assert_eq!(IncludesAddressResponse { included: true }, res);
    }
}
