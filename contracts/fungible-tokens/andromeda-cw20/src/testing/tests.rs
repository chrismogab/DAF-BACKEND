use crate::contract::{execute, instantiate, query};
use andromeda_fungible_tokens::{cw20::{ExecuteMsg, InstantiateMsg, QueryMsg, GetTokenPriceResponse}};
use andromeda_modules::receipt::{ExecuteMsg as ReceiptExecuteMsg, Receipt};
use andromeda_testing::testing::mock_querier::{
    mock_dependencies_custom, MOCK_ADDRESSLIST_CONTRACT, MOCK_RATES_CONTRACT, MOCK_RECEIPT_CONTRACT,
};
use common::{
    ado_base::{
        modules::{Module, ADDRESS_LIST, RATES, RECEIPT},
        AndromedaMsg, AndromedaQuery,
    },
    app::AndrAddress,
    error::ContractError,
};
use cosmwasm_std::{
    testing::{mock_env, mock_info, mock_dependencies_with_balance},
    to_binary, Addr, CosmosMsg, Event, Response, StdError, SubMsg, Uint128, WasmMsg, DepsMut, coins, from_binary,
};
use cw20::{Cw20Coin, Cw20ReceiveMsg, AllowanceResponse, };
use cw20_base::{state::{BALANCES}, allowances};

fn init(
    _deps: DepsMut,
) -> Result<Response, ContractError> {
    let info = mock_info("owner", &[]);

    let msg = InstantiateMsg {
        name: "DAF".into(),
        symbol: "symbol".into(),
        decimals: 6,
        token_price:  2u128.into(),
        initial_balances: vec![Cw20Coin {
            amount: 1000u128.into(),
            address: "sender".to_string(),
        }],
        mint: None,
        marketing: None,
        modules: None,
        usdc_address: "usdc".into(),
        owner: "sender".into(),
    };
    instantiate(_deps, mock_env(), info, msg)
}

#[test]
fn test_andr_query() {
    let mut deps = mock_dependencies_custom(&[]);
    let info = mock_info("owner", &[]);

    let instantiate_msg = InstantiateMsg {
        token_price: Uint128::from(1u128),
        name: "Name".into(),
        symbol: "Symbol".into(),
        decimals: 6,
        initial_balances: vec![Cw20Coin {
            amount: 1000u128.into(),
            address: "sender".to_string(),
        }],
        mint: None,
        marketing: None,  
        modules: None,
        usdc_address: "usdc".into(),
        owner: "sender".into(),

    };

    let _res = instantiate(deps.as_mut(), mock_env(), info, instantiate_msg).unwrap();

    let msg = QueryMsg::AndrQuery(AndromedaQuery::Owner {});
    let res = query(deps.as_ref(), mock_env(), msg);
    // Test that the query is hooked up correctly.
    assert!(res.is_ok())
}

#[test]
fn test_update_price_works() {
    let mut deps = mock_dependencies_custom(&[]);
    let instantiate_msg = InstantiateMsg {
        token_price: Uint128::from(1u128),
        name: "Name".into(),
        symbol: "Symbol".into(),
        decimals: 6,
        initial_balances: vec![Cw20Coin {
            amount: 1000u128.into(),
            address: "sender".to_string(),
        }],
        mint: None,
        marketing: None,  
        modules: None,
        usdc_address: "usdc".into(),
        owner: "sender".into(),

    };
    let info = mock_info("sender", &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), instantiate_msg).unwrap();
    let execute_msg = ExecuteMsg::UpdatePrice { new_price:  Uint128::from(1u128) };
    let _ex_msg = execute(deps.as_mut(), mock_env(), info.clone(), execute_msg).unwrap();

    let query_msg = QueryMsg::GetTokenPrice {  };
    //it works, let's query the state
    let q_res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let value: GetTokenPriceResponse =  from_binary(&q_res).unwrap();
    assert_eq!(Uint128::from(1u128), value.price);
    
}

#[test]
#[should_panic]
fn test_update_price_fails() {
    let mut deps = mock_dependencies_custom(&[]);
    let instantiate_msg = InstantiateMsg {
        token_price: Uint128::from(1u128),
        name: "Name".into(),
        symbol: "Symbol".into(),
        decimals: 6,
        initial_balances: vec![Cw20Coin {
            amount: 1000u128.into(),
            address: "sender".to_string(),
        }],
        mint: None,
        marketing: None,  
        modules: None,
        usdc_address: "usdc".into(),
        owner: "sender".into(),
    };
    let info = mock_info("sender", &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), instantiate_msg).unwrap();
    let execute_msg2 = ExecuteMsg::UpdatePrice { new_price:  Uint128::from(1u128) };
    let info = mock_info("sender2", &vec![]);
    let _ex_msg = execute(deps.as_mut(), mock_env(), info.clone(), execute_msg2).unwrap();
    
    let query_msg = QueryMsg::GetTokenPrice {  };
    let q_res2 = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let value: GetTokenPriceResponse = from_binary(&q_res2).unwrap();
    assert_eq!(Uint128::from(1u128), value.price);

    }


    #[test]
    fn test_transfer() {
        let modules: Vec<Module> = vec![
            Module {
                module_type: RECEIPT.to_owned(),
                address: AndrAddress {
                    identifier: MOCK_RECEIPT_CONTRACT.to_owned(),
                },
                is_mutable: false,
            },
            Module {
                module_type: RATES.to_owned(),
                address: AndrAddress {
                    identifier: MOCK_RATES_CONTRACT.to_owned(),
                },
                is_mutable: false,
            },
            Module {
                module_type: ADDRESS_LIST.to_owned(),
                address: AndrAddress {
                    identifier: MOCK_ADDRESSLIST_CONTRACT.to_owned(),
                },
                is_mutable: false,
            },
        ];
    
        let mut deps = mock_dependencies_custom(&[]);
        let info = mock_info("sender", &[]);
    
        let instantiate_msg = InstantiateMsg {
            token_price: Uint128::from(1u128),
            name: "Name".into(),
            symbol: "Symbol".into(),
            decimals: 6,
            initial_balances: vec![Cw20Coin {
                amount: 1000u128.into(),
                address: "sender".to_string(),
            }],
            mint: None,
            marketing: None,
            modules: Some(modules),
            usdc_address: "USDC".into(),
            owner: "owner".into(),
        };
    
        let res = instantiate(deps.as_mut(), mock_env(), info.clone(), instantiate_msg).unwrap();
        assert_eq!(
            Response::new()
                .add_attribute("action", "register_module")
                .add_attribute("module_idx", "1")
                .add_attribute("action", "register_module")
                .add_attribute("module_idx", "2")
                .add_attribute("action", "register_module")
                .add_attribute("module_idx", "3")
                .add_attribute("method", "instantiate")
                .add_attribute("type", "cw20"),
            res
        );
    
        assert_eq!(
            Uint128::from(1000u128),
            BALANCES
                .load(deps.as_ref().storage, &Addr::unchecked("sender"))
                .unwrap()
        );
    
        let msg = ExecuteMsg::Transfer {
            recipient: "other".into(),
            amount: 100u128.into(),
        };
    
        let not_whitelisted_info = mock_info("not_whitelisted", &[]);
        let res = execute(deps.as_mut(), mock_env(), not_whitelisted_info, msg.clone());
        assert_eq!(
            ContractError::Std(StdError::generic_err(
                "Querier contract error: InvalidAddress"
            )),
            res.unwrap_err()
        );
    
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    
        let receipt_msg: SubMsg = SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: MOCK_RECEIPT_CONTRACT.to_string(),
            msg: to_binary(&ReceiptExecuteMsg::StoreReceipt {
                receipt: Receipt {
                    events: vec![Event::new("Royalty"), Event::new("Tax")],
                },
            })
            .unwrap(),
            funds: vec![],
        }));
    
        assert_eq!(
            Response::new()
                .add_submessage(receipt_msg)
                .add_event(Event::new("Royalty"))
                .add_event(Event::new("Tax"))
                .add_attribute("action", "transfer")
                .add_attribute("from", "sender")
                .add_attribute("to", "other")
                .add_attribute("amount", "90"),
            res
        );
    
        // Funds deducted from the sender (100 for send, 10 for tax).
        assert_eq!(
            Uint128::from(890u128),
            BALANCES
                .load(deps.as_ref().storage, &Addr::unchecked("sender"))
                .unwrap()
        );
    
        // Funds given to the receiver.
        assert_eq!(
            Uint128::from(90u128),
            BALANCES
                .load(deps.as_ref().storage, &Addr::unchecked("other"))
                .unwrap()
        );
    
        // Royalty given to rates_recipient
        assert_eq!(
            Uint128::from(20u128),
            BALANCES
                .load(deps.as_ref().storage, &Addr::unchecked("rates_recipient"))
                .unwrap()
        );
    }
    
#[test]
fn test_error_self_allowance() {
    let mut deps = mock_dependencies_with_balance(&coins(2, "token"));
    let owner = String::from("address_of_owner");
    let info = mock_info(owner.as_ref(), &[]);
    let env = mock_env();
    init(deps.as_mut()).unwrap();

    let msg = ExecuteMsg::IncreaseAllowancePurchase {
         spender: owner.clone(), 
         amount: Uint128::from(10u128),
         owner: owner,
         };
    let err = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap_err();
    assert_eq!(err, ContractError::CannotSetOwnAccount {});

}

#[test]
fn test_increase_allowance_purchase(){
    let mut deps = mock_dependencies_with_balance(&coins(2, "token"));
    let owner = String::from("owner_address");
    let spender = String::from("spender_address");
    let info = mock_info(owner.as_ref(), &[]);
    let env = mock_env();
    init(deps.as_mut()).unwrap();

    
    //0 allowance
    let allowance = allowances::query_allowance(deps.as_ref(), owner.clone(), spender.clone()).unwrap();
    assert_eq!(allowance, AllowanceResponse::default());

    // set allowance
    let allowance1 = Uint128::new(100);
    let expires = cw20::Expiration::Never {};

    let msg = ExecuteMsg::IncreaseAllowancePurchase { 
        spender: spender.clone(),
        amount: allowance1,
        owner: owner.clone(),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    

    // query and make sure it works
    let allowance = allowances::query_allowance(deps.as_ref(), owner.clone(), spender.clone()).unwrap();
    assert_eq!(
        allowance,
        AllowanceResponse {
            allowance: allowance1,
            expires

        }
    );
}

#[test]
fn test_send() {
    let modules: Vec<Module> = vec![
        Module {
            module_type: RECEIPT.to_owned(),
            address: AndrAddress {
                identifier: MOCK_RECEIPT_CONTRACT.to_owned(),
            },
            is_mutable: false,
        },
        Module {
            module_type: RATES.to_owned(),
            address: AndrAddress {
                identifier: MOCK_RATES_CONTRACT.to_owned(),
            },
            is_mutable: false,
        },
        Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: MOCK_ADDRESSLIST_CONTRACT.to_owned(),
            },
            is_mutable: false,
        },
    ];

    let mut deps = mock_dependencies_custom(&[]);
    let info = mock_info("sender", &[]);

    let instantiate_msg = InstantiateMsg {
        token_price: Uint128::from(1u128),
        name: "Name".into(),
        symbol: "Symbol".into(),
        decimals: 6,
        initial_balances: vec![Cw20Coin {
            amount: 1000u128.into(),
            address: "sender".to_string(),
        }],
        mint: None,
        marketing: None,
        modules: Some(modules),
        usdc_address: "usdc".into(),
        owner: "sender".into(),

    };

    let res = instantiate(deps.as_mut(), mock_env(), info.clone(), instantiate_msg).unwrap();
    assert_eq!(
        Response::new()
            .add_attribute("action", "register_module")
            .add_attribute("module_idx", "1")
            .add_attribute("action", "register_module")
            .add_attribute("module_idx", "2")
            .add_attribute("action", "register_module")
            .add_attribute("module_idx", "3")
            .add_attribute("method", "instantiate")
            .add_attribute("type", "cw20"),
        res
    );

    assert_eq!(
        Uint128::from(1000u128),
        BALANCES
            .load(deps.as_ref().storage, &Addr::unchecked("sender"))
            .unwrap()
    );

    let msg = ExecuteMsg::Send {
        contract: "contract".into(),
        amount: 100u128.into(),
        msg: to_binary(&"msg").unwrap(),
    };

    let not_whitelisted_info = mock_info("not_whitelisted", &[]);
    let res = execute(deps.as_mut(), mock_env(), not_whitelisted_info, msg.clone());
    assert_eq!(
        ContractError::Std(StdError::generic_err(
            "Querier contract error: InvalidAddress"
        )),
        res.unwrap_err()
    );

    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let receipt_msg: SubMsg = SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: MOCK_RECEIPT_CONTRACT.to_string(),
        msg: to_binary(&ReceiptExecuteMsg::StoreReceipt {
            receipt: Receipt {
                events: vec![Event::new("Royalty"), Event::new("Tax")],
            },
        })
        .unwrap(),
        funds: vec![],
    }));

    assert_eq!(
        Response::new()
            .add_submessage(receipt_msg)
            .add_event(Event::new("Royalty"))
            .add_event(Event::new("Tax"))
            .add_attribute("action", "send")
            .add_attribute("from", "sender")
            .add_attribute("to", "contract")
            .add_attribute("amount", "90")
            .add_message(
                Cw20ReceiveMsg {
                    sender: "sender".into(),
                    amount: 90u128.into(),
                    msg: to_binary(&"msg").unwrap(),
                }
                .into_cosmos_msg("contract")
                .unwrap(),
            ),
        res
    );

    // Funds deducted from the sender (100 for send, 10 for tax).
    assert_eq!(
        Uint128::from(890u128),
        BALANCES
            .load(deps.as_ref().storage, &Addr::unchecked("sender"))
            .unwrap()
    );

    // Funds given to the receiver.
    assert_eq!(
        Uint128::from(90u128),
        BALANCES
            .load(deps.as_ref().storage, &Addr::unchecked("contract"))
            .unwrap()
    );

    // Royalty given to rates_recipient (10 from royalty and 10 from tax)
    assert_eq!(
        Uint128::from(20u128),
        BALANCES
            .load(deps.as_ref().storage, &Addr::unchecked("rates_recipient"))
            .unwrap()
    );
}

#[test]
fn test_update_app_contract() {
    let mut deps = mock_dependencies_custom(&[]);

    let modules: Vec<Module> = vec![Module {
        module_type: ADDRESS_LIST.to_owned(),
        address: AndrAddress {
            identifier: MOCK_ADDRESSLIST_CONTRACT.to_owned(),
        },
        is_mutable: false,
    }];

    let info = mock_info("app_contract", &[]);
    let instantiate_msg = InstantiateMsg {
        token_price: Uint128::from(1u128),
        name: "Name".into(),
        symbol: "Symbol".into(),
        decimals: 6,
        initial_balances: vec![Cw20Coin {
            amount: 1000u128.into(),
            address: "sender".to_string(),
        }],
        mint: None,
        marketing: None,
        modules: Some(modules),
        usdc_address: "usdc".into(),
        owner: "sender".into(),

    };

    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), instantiate_msg).unwrap();

    let msg = ExecuteMsg::AndrReceive(AndromedaMsg::UpdateAppContract {
        address: "app_contract".to_string(),
    });

    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    assert_eq!(
        Response::new()
            .add_attribute("action", "update_app_contract")
            .add_attribute("address", "app_contract"),
        res
    );
}
