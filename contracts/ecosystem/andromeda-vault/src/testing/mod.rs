mod mock_querier;

use crate::contract::*;
use crate::testing::mock_querier::{mock_dependencies_custom, PositionResponse};
use andromeda_ecosystem::vault::{
    ExecuteMsg, InstantiateMsg, QueryMsg, StrategyAddressResponse, StrategyType, YieldStrategy,
    BALANCES, STRATEGY_CONTRACT_ADDRESSES,
};
use common::{
    ado_base::{recipient::Recipient, AndromedaMsg},
    app::AndrAddress,
    error::ContractError,
    withdraw::{Withdrawal, WithdrawalType},
};
use cosmwasm_std::{
    coin, from_binary,
    testing::{mock_dependencies, mock_env, mock_info},
    to_binary, wasm_execute, Addr, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo,
    ReplyOn, Response, SubMsg, Uint128, WasmMsg,
};

use self::mock_querier::{MOCK_ANCHOR_CONTRACT, MOCK_VAULT_CONTRACT};

#[test]
fn test_instantiate() {
    let inst_msg = InstantiateMsg {};
    let env = mock_env();
    let info = mock_info("minter", &[]);
    let mut deps = mock_dependencies();

    instantiate(deps.as_mut(), env, info, inst_msg).unwrap();
}

#[test]
fn test_deposit() {
    let env = mock_env();
    let sent_funds = coin(100, "uusd");
    let extra_sent_funds = coin(100, "uluna");
    let depositor = "depositor".to_string();
    let mut deps = mock_dependencies();

    let info = mock_info(&depositor, &[sent_funds.clone(), extra_sent_funds.clone()]);
    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: None,
        strategy: None,
    };

    execute(deps.as_mut(), env, info, msg).unwrap();

    let uusd_balance = BALANCES
        .load(deps.as_ref().storage, (&depositor, "uusd"))
        .unwrap();
    assert_eq!(uusd_balance, sent_funds.amount);
    let uluna_balance = BALANCES
        .load(deps.as_ref().storage, (&depositor, "uluna"))
        .unwrap();
    assert_eq!(uluna_balance, extra_sent_funds.amount)
}

fn add_strategy(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    strategy: StrategyType,
    address: AndrAddress,
) -> Response {
    let msg = ExecuteMsg::UpdateStrategy { strategy, address };
    execute(deps, env, info, msg).unwrap()
}

#[test]
fn test_execute_update_strategy() {
    let mut env = mock_env();
    let depositor = "depositor".to_string();
    let mut deps = mock_dependencies_custom(&[]);
    let inst_msg = InstantiateMsg {};
    let info = mock_info(&depositor, &[]);
    instantiate(deps.as_mut(), env.clone(), info.clone(), inst_msg).unwrap();
    env.contract.address = Addr::unchecked(MOCK_VAULT_CONTRACT);
    let resp = add_strategy(
        deps.as_mut(),
        env,
        info,
        StrategyType::Anchor,
        AndrAddress {
            identifier: MOCK_ANCHOR_CONTRACT.to_string(),
        },
    );

    let expected = Response::default()
        .add_attribute("action", "update_strategy")
        .add_attribute("strategy_type", StrategyType::Anchor.to_string())
        .add_attribute("addr", MOCK_ANCHOR_CONTRACT.to_string());

    assert_eq!(resp, expected);

    let addr = STRATEGY_CONTRACT_ADDRESSES
        .load(deps.as_mut().storage, StrategyType::Anchor.to_string())
        .unwrap();
    assert_eq!(addr, MOCK_ANCHOR_CONTRACT.to_string());
}

#[test]
fn test_execute_update_strategy_not_operator() {
    let mut env = mock_env();
    let depositor = "depositor".to_string();
    let mut deps = mock_dependencies_custom(&[]);
    let inst_msg = InstantiateMsg {};
    let info = mock_info(&depositor, &[]);
    instantiate(deps.as_mut(), env.clone(), info.clone(), inst_msg).unwrap();
    env.contract.address = Addr::unchecked("someinvalidvaultaddress");
    let msg = ExecuteMsg::UpdateStrategy {
        strategy: StrategyType::Anchor,
        address: AndrAddress {
            identifier: MOCK_ANCHOR_CONTRACT.to_string(),
        },
    };
    let resp = execute(deps.as_mut(), env, info, msg).unwrap_err();

    let expected = ContractError::NotAssignedOperator {
        msg: Some("Vault contract is not an operator for the given address".to_string()),
    };

    assert_eq!(resp, expected);
}

#[test]
fn test_deposit_insufficient_funds() {
    let env = mock_env();
    let depositor = "depositor".to_string();
    let mut deps = mock_dependencies();

    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: None,
        strategy: None,
    };

    let err = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    assert_eq!(ContractError::InsufficientFunds {}, err);

    let info_with_funds = mock_info(&depositor, &[coin(100, "uusd")]);
    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: Some(coin(0u128, "uusd")),
        strategy: None,
    };

    let err = execute(deps.as_mut(), env.clone(), info_with_funds.clone(), msg).unwrap_err();
    assert_eq!(ContractError::InsufficientFunds {}, err);

    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: Some(coin(150u128, "uusd")),
        strategy: None,
    };

    let err = execute(deps.as_mut(), env, info_with_funds, msg).unwrap_err();
    assert_eq!(ContractError::InsufficientFunds {}, err)
}

#[test]
fn test_deposit_strategy() {
    let yield_strategy = YieldStrategy {
        strategy_type: StrategyType::Anchor,
        address: AndrAddress {
            identifier: MOCK_ANCHOR_CONTRACT.to_string(),
        },
    };
    let inst_msg = InstantiateMsg {};
    let mut env = mock_env();
    let info = mock_info("minter", &[]);
    let mut deps = mock_dependencies_custom(&[]);

    instantiate(deps.as_mut(), env.clone(), info.clone(), inst_msg).unwrap();
    env.contract.address = Addr::unchecked(MOCK_VAULT_CONTRACT);
    add_strategy(
        deps.as_mut(),
        env.clone(),
        info,
        yield_strategy.clone().strategy_type,
        yield_strategy.clone().address,
    );

    let sent_funds = coin(100, "uusd");
    let extra_sent_funds = coin(100, "uluna");
    let funds = vec![sent_funds.clone(), extra_sent_funds.clone()];
    let depositor = "depositor".to_string();
    let info = mock_info(&depositor, &funds);
    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: None,
        strategy: Some(yield_strategy.clone().strategy_type),
    };
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    let recipient = Recipient::Addr("depositor".to_string());

    let msg = wasm_execute(
        yield_strategy
            .address
            .get_address(deps.as_ref().api, &deps.as_ref().querier, None)
            .unwrap(),
        &ExecuteMsg::AndrReceive(AndromedaMsg::Receive(Some(to_binary(&recipient).unwrap()))),
        vec![sent_funds],
    )
    .unwrap();
    let msg_two = wasm_execute(
        yield_strategy
            .address
            .get_address(deps.as_ref().api, &deps.as_ref().querier, None)
            .unwrap(),
        &ExecuteMsg::AndrReceive(AndromedaMsg::Receive(Some(to_binary(&recipient).unwrap()))),
        vec![extra_sent_funds],
    )
    .unwrap();
    let deposit_submsg = SubMsg {
        id: 1,
        msg: CosmosMsg::Wasm(msg),
        gas_limit: None,
        reply_on: ReplyOn::Error,
    };
    let deposit_submsg_two = SubMsg {
        id: 1,
        msg: CosmosMsg::Wasm(msg_two),
        gas_limit: None,
        reply_on: ReplyOn::Error,
    };
    let expected = Response::default()
        .add_submessage(deposit_submsg)
        .add_submessage(deposit_submsg_two);

    assert_eq!(expected, res)
}

#[test]
fn test_deposit_strategy_partial_amount() {
    let yield_strategy = YieldStrategy {
        strategy_type: StrategyType::Anchor,
        address: AndrAddress {
            identifier: MOCK_ANCHOR_CONTRACT.to_string(),
        },
    };
    let inst_msg = InstantiateMsg {};
    let mut env = mock_env();
    let info = mock_info("minter", &[]);
    let mut deps = mock_dependencies_custom(&[]);

    instantiate(deps.as_mut(), env.clone(), info.clone(), inst_msg).unwrap();
    env.contract.address = Addr::unchecked(MOCK_VAULT_CONTRACT);
    add_strategy(
        deps.as_mut(),
        env.clone(),
        info,
        yield_strategy.clone().strategy_type,
        yield_strategy.clone().address,
    );

    let sent_funds = coin(90, "uusd");
    let funds = vec![sent_funds.clone()];
    BALANCES
        .save(
            deps.as_mut().storage,
            ("depositor", &sent_funds.denom),
            &Uint128::from(20u128),
        )
        .unwrap();

    let depositor = "depositor".to_string();
    let info = mock_info(&depositor, &funds);
    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: Some(coin(100, sent_funds.denom.clone())),
        strategy: Some(yield_strategy.clone().strategy_type),
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    let msg = wasm_execute(
        yield_strategy
            .address
            .get_address(deps.as_ref().api, &deps.as_ref().querier, None)
            .unwrap(),
        &ExecuteMsg::AndrReceive(AndromedaMsg::Receive(Some(
            to_binary(&Recipient::Addr("depositor".to_string())).unwrap(),
        ))),
        vec![coin(100, sent_funds.denom.clone())],
    )
    .unwrap();
    let deposit_submsg = SubMsg {
        id: 1,
        msg: CosmosMsg::Wasm(msg),
        gas_limit: None,
        reply_on: ReplyOn::Error,
    };
    let expected = Response::default().add_submessage(deposit_submsg);

    assert_eq!(expected, res);

    let post_balance = BALANCES
        .load(deps.as_ref().storage, ("depositor", &sent_funds.denom))
        .unwrap();

    assert_eq!(Uint128::from(10u128), post_balance);
}

#[test]
fn test_deposit_strategy_empty_funds_non_empty_amount() {
    let env = mock_env();
    let mut deps = mock_dependencies();

    let depositor = "depositor".to_string();
    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: Some(coin(100, "uusd")),
        strategy: None,
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(ContractError::InsufficientFunds {}, err);
}

#[test]
fn test_deposit_strategy_insufficient_partial_amount() {
    let yield_strategy = YieldStrategy {
        strategy_type: StrategyType::Anchor,
        address: AndrAddress {
            identifier: "terra1anchoraddress".to_string(),
        },
    };
    let inst_msg = InstantiateMsg {};
    let env = mock_env();
    let info = mock_info("minter", &[]);
    let mut deps = mock_dependencies();

    instantiate(deps.as_mut(), env.clone(), info, inst_msg).unwrap();

    let sent_funds = coin(90, "uusd");
    let funds = vec![sent_funds.clone()];
    BALANCES
        .save(
            deps.as_mut().storage,
            ("depositor", &sent_funds.denom),
            &Uint128::from(5u128),
        )
        .unwrap();

    let depositor = "depositor".to_string();
    let info = mock_info(&depositor, &funds);
    let msg = ExecuteMsg::Deposit {
        recipient: None,
        amount: Some(coin(100, sent_funds.denom.clone())),
        strategy: Some(yield_strategy.strategy_type),
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(ContractError::InsufficientFunds {}, err);

    let post_balance = BALANCES
        .load(deps.as_ref().storage, ("depositor", &sent_funds.denom))
        .unwrap();

    assert_eq!(Uint128::from(5u128), post_balance);
}

#[test]
fn test_withdraw_empty() {
    let env = mock_env();
    let mut deps = mock_dependencies();

    let depositor = "depositor".to_string();
    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![],
        strategy: None,
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(
        ContractError::InvalidTokensToWithdraw {
            msg: "No tokens provided for withdrawal".to_string()
        },
        err
    );
}

#[test]
fn test_withdraw_invalid_withdrawals() {
    let env = mock_env();
    let mut deps = mock_dependencies();

    let depositor = "depositor".to_string();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uusd"),
            &Uint128::from(100u128),
        )
        .unwrap();
    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![Withdrawal {
            token: "uusd".to_string(),
            withdrawal_type: Some(WithdrawalType::Amount(Uint128::zero())),
        }],
        strategy: None,
    };

    let err = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap_err();
    assert_eq!(
        ContractError::InvalidWithdrawal {
            msg: Some("Amount must be non-zero".to_string())
        },
        err
    );

    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![Withdrawal {
            token: "uusd".to_string(),
            withdrawal_type: Some(WithdrawalType::Percentage(Decimal::zero())),
        }],
        strategy: None,
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(
        ContractError::InvalidWithdrawal {
            msg: Some("Percent must be non-zero".to_string())
        },
        err
    );
}

#[test]
fn test_withdraw_single_no_strategy_insufficientfunds() {
    let env = mock_env();
    let mut deps = mock_dependencies();

    let depositor = "depositor".to_string();
    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![Withdrawal {
            token: "uusd".to_string(),
            withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(100u128))),
        }],
        strategy: None,
    };

    let err = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap_err();
    assert_eq!(ContractError::InsufficientFunds {}, err);

    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uusd"),
            &Uint128::from(75u128),
        )
        .unwrap();

    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![Withdrawal {
            token: "uusd".to_string(),
            withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(100u128))),
        }],
        strategy: None,
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(ContractError::InsufficientFunds {}, err);
}

#[test]
fn test_withdraw_single_no_strategy_amount() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let depositor = "depositor".to_string();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uusd"),
            &Uint128::from(150u128),
        )
        .unwrap();

    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![Withdrawal {
            token: "uusd".to_string(),
            withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(100u128))),
        }],
        strategy: None,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    let expected = Response::default().add_message(CosmosMsg::Bank(BankMsg::Send {
        to_address: depositor.clone(),
        amount: vec![coin(100, "uusd")],
    }));

    assert_eq!(expected, res);

    let uusd_balance = BALANCES
        .load(deps.as_mut().storage, (&depositor, "uusd"))
        .unwrap_or_else(|_| Uint128::zero());
    assert_eq!(Uint128::from(50u128), uusd_balance);
}

#[test]
fn test_withdraw_single_no_strategy_percentage() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let depositor = "depositor".to_string();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uusd"),
            &Uint128::from(150u128),
        )
        .unwrap();

    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![Withdrawal {
            token: "uusd".to_string(),
            withdrawal_type: Some(WithdrawalType::Percentage(Decimal::percent(50))),
        }],
        strategy: None,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    let expected = Response::default().add_message(CosmosMsg::Bank(BankMsg::Send {
        to_address: depositor.clone(),
        amount: vec![coin(75, "uusd")],
    }));

    assert_eq!(expected, res);

    let uusd_balance = BALANCES
        .load(deps.as_mut().storage, (&depositor, "uusd"))
        .unwrap_or_else(|_| Uint128::zero());
    assert_eq!(Uint128::from(75u128), uusd_balance);
}

#[test]
fn test_withdraw_multi_no_strategy_insufficientfunds() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let depositor = "depositor".to_string();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uusd"),
            &Uint128::from(75u128),
        )
        .unwrap();

    let depositor = "depositor".to_string();
    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![
            Withdrawal {
                token: "uusd".to_string(),
                withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(50u128))),
            },
            Withdrawal {
                token: "uluna".to_string(),
                withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(50u128))),
            },
        ],
        strategy: None,
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(ContractError::InsufficientFunds {}, err);
}

#[test]
fn test_withdraw_multi_no_strategy_mixed() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let depositor = "depositor".to_string();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uusd"),
            &Uint128::from(150u128),
        )
        .unwrap();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uluna"),
            &Uint128::from(150u128),
        )
        .unwrap();

    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: vec![
            Withdrawal {
                token: "uusd".to_string(),
                withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(100u128))),
            },
            Withdrawal {
                token: "uusd".to_string(),
                withdrawal_type: None,
            },
            Withdrawal {
                token: "uluna".to_string(),
                withdrawal_type: Some(WithdrawalType::Percentage(Decimal::one())),
            },
        ],
        strategy: None,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    let expected = Response::default().add_message(CosmosMsg::Bank(BankMsg::Send {
        to_address: depositor.clone(),
        amount: vec![coin(100, "uusd"), coin(50, "uusd"), coin(150, "uluna")],
    }));

    assert_eq!(expected, res);

    let uusd_balance = BALANCES
        .load(deps.as_mut().storage, (&depositor, "uusd"))
        .unwrap_or_else(|_| Uint128::zero());
    assert!(uusd_balance.is_zero());
    let uluna_balance = BALANCES
        .load(deps.as_mut().storage, (&depositor, "uluna"))
        .unwrap_or_else(|_| Uint128::zero());
    assert!(uluna_balance.is_zero());
}

#[test]
fn test_withdraw_multi_no_strategy_recipient() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let depositor = "depositor".to_string();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uusd"),
            &Uint128::from(150u128),
        )
        .unwrap();
    BALANCES
        .save(
            deps.as_mut().storage,
            (&depositor, "uluna"),
            &Uint128::from(150u128),
        )
        .unwrap();

    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: Some(Recipient::Addr("recipient".to_string())),
        withdrawals: vec![
            Withdrawal {
                token: "uusd".to_string(),
                withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(100u128))),
            },
            Withdrawal {
                token: "uusd".to_string(),
                withdrawal_type: Some(WithdrawalType::Percentage(Decimal::one())),
            },
            Withdrawal {
                token: "uluna".to_string(),
                withdrawal_type: Some(WithdrawalType::Percentage(Decimal::one())),
            },
        ],
        strategy: None,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    let expected = Response::default().add_message(CosmosMsg::Bank(BankMsg::Send {
        to_address: "recipient".to_string(),
        amount: vec![coin(100, "uusd"), coin(50, "uusd"), coin(150, "uluna")],
    }));

    assert_eq!(expected, res);

    let uusd_balance = BALANCES
        .load(deps.as_mut().storage, (&depositor, "uusd"))
        .unwrap_or_else(|_| Uint128::zero());
    assert!(uusd_balance.is_zero());
    let uluna_balance = BALANCES
        .load(deps.as_mut().storage, (&depositor, "uluna"))
        .unwrap_or_else(|_| Uint128::zero());
    assert!(uluna_balance.is_zero());
}

#[test]
fn test_withdraw_single_strategy() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let depositor = "depositor".to_string();
    STRATEGY_CONTRACT_ADDRESSES
        .save(
            deps.as_mut().storage,
            StrategyType::Anchor.to_string(),
            &MOCK_ANCHOR_CONTRACT.to_string(),
        )
        .unwrap();
    let withdrawals = vec![Withdrawal {
        token: "aust".to_string(),
        withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(100u128))),
    }];

    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals: withdrawals.clone(),
        strategy: Some(StrategyType::Anchor),
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    let withdraw_exec = to_binary(&ExecuteMsg::AndrReceive(AndromedaMsg::Withdraw {
        recipient: Some(Recipient::Addr("depositor".to_string())),
        tokens_to_withdraw: Some(withdrawals),
    }))
    .unwrap();
    let withdraw_submsg = SubMsg {
        id: 104,
        msg: CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: MOCK_ANCHOR_CONTRACT.to_string(),
            msg: withdraw_exec,
            funds: vec![],
        }),
        gas_limit: None,
        reply_on: ReplyOn::Error,
    };
    let expected = Response::default().add_submessage(withdraw_submsg);

    assert_eq!(expected, res);
}

#[test]
fn test_withdraw_invalid_strategy() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let depositor = "depositor".to_string();
    let withdrawals = vec![Withdrawal {
        token: "aust".to_string(),
        withdrawal_type: Some(WithdrawalType::Amount(Uint128::from(100u128))),
    }];

    let info = mock_info(&depositor, &[]);
    let msg = ExecuteMsg::Withdraw {
        recipient: None,
        withdrawals,
        strategy: Some(StrategyType::Anchor),
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(
        ContractError::InvalidStrategy {
            strategy: StrategyType::Anchor.to_string()
        },
        err
    );
}

#[test]
fn test_query_local_balance() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let depositor = "depositor";
    let balance_one = coin(100, "uluna");
    let balance_two = coin(200, "uusd");
    BALANCES
        .save(
            deps.as_mut().storage,
            (depositor, &balance_one.denom),
            &balance_one.amount.clone(),
        )
        .unwrap();
    BALANCES
        .save(
            deps.as_mut().storage,
            (depositor, &balance_two.denom),
            &balance_two.amount.clone(),
        )
        .unwrap();

    let single_query = QueryMsg::Balance {
        address: depositor.to_string(),
        strategy: None,
        denom: Some(balance_one.denom.clone()),
    };

    let resp = query(deps.as_ref(), env.clone(), single_query).unwrap();
    let balance: Vec<Coin> = from_binary(&resp).unwrap();
    assert_eq!(1, balance.len());
    assert_eq!(balance_one, balance[0]);

    let multi_query = QueryMsg::Balance {
        address: depositor.to_string(),
        strategy: None,
        denom: None,
    };

    let resp = query(deps.as_ref(), env, multi_query).unwrap();
    let balance: Vec<Coin> = from_binary(&resp).unwrap();
    assert_eq!(2, balance.len());
    assert_eq!(balance_one, balance[0]);
    assert_eq!(balance_two, balance[1]);
}

#[test]
fn test_query_strategy_balance() {
    let mut deps = mock_dependencies_custom(&[]);
    let env = mock_env();
    let depositor = "depositor";

    STRATEGY_CONTRACT_ADDRESSES
        .save(
            deps.as_mut().storage,
            StrategyType::Anchor.to_string(),
            &MOCK_ANCHOR_CONTRACT.to_string(),
        )
        .unwrap();

    let single_query = QueryMsg::Balance {
        address: depositor.to_string(),
        strategy: Some(StrategyType::Anchor),
        denom: None,
    };

    let resp = query(deps.as_ref(), env, single_query).unwrap();
    let balance: PositionResponse = from_binary(&resp).unwrap();
    assert_eq!(Uint128::from(10u128), balance.aust_amount);
    assert_eq!(
        "depositor".to_string(),
        balance
            .recipient
            .get_addr(deps.as_ref().api, &deps.as_ref().querier, None)
            .unwrap()
    );
}

#[test]
fn test_query_strategy_address() {
    let mut deps = mock_dependencies_custom(&[]);
    let env = mock_env();

    STRATEGY_CONTRACT_ADDRESSES
        .save(
            deps.as_mut().storage,
            StrategyType::Anchor.to_string(),
            &MOCK_ANCHOR_CONTRACT.to_string(),
        )
        .unwrap();

    let single_query = QueryMsg::StrategyAddress {
        strategy: StrategyType::Anchor,
    };

    let resp = query(deps.as_ref(), env, single_query).unwrap();
    let addr_resp: StrategyAddressResponse = from_binary(&resp).unwrap();
    assert_eq!(MOCK_ANCHOR_CONTRACT.to_string(), addr_resp.address);
    assert_eq!(StrategyType::Anchor, addr_resp.strategy);
}

#[test]
fn test_query_strategy_address_invalid() {
    let deps = mock_dependencies_custom(&[]);
    let env = mock_env();

    let single_query = QueryMsg::StrategyAddress {
        strategy: StrategyType::Anchor,
    };

    let err = query(deps.as_ref(), env, single_query).unwrap_err();
    assert_eq!(
        ContractError::InvalidStrategy {
            strategy: StrategyType::Anchor.to_string()
        },
        err
    );
}
