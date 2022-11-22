use std::convert::TryInto;

use crate::state::ADOContract;
use cosmwasm_std::{Api, Order, QuerierWrapper, Response, Storage, Uint64};
use cw_storage_plus::Bound;

use common::{ado_base::modules::Module, error::ContractError};

pub mod execute;
pub mod hooks;
pub mod query;

impl<'a> ADOContract<'a> {
    pub(crate) fn register_modules(
        &self,
        sender: &str,
        storage: &mut dyn Storage,
        modules: Vec<Module>,
    ) -> Result<Response, ContractError> {
        self.validate_modules(&modules, &self.ado_type.load(storage)?)?;
        let mut resp = Response::new();
        for module in modules {
            let register_response = self.execute_register_module(storage, sender, module, false)?;
            resp = resp
                .add_attributes(register_response.attributes)
                .add_submessages(register_response.messages)
        }
        Ok(resp)
    }

    /// Registers a module
    /// If the module has provided an address as its form of instantiation this address is recorded
    /// Each module is assigned a u64 index so as it can be unregistered/altered
    /// The assigned u64 index is used as the message id for use in the `reply` entry point of the contract
    fn register_module(
        &self,
        storage: &mut dyn Storage,
        module: &Module,
    ) -> Result<u64, ContractError> {
        let idx = self.module_idx.may_load(storage)?.unwrap_or(1);
        let idx_str = idx.to_string();
        self.module_info.save(storage, &idx_str, module)?;
        self.module_idx.save(storage, &(idx + 1))?;

        Ok(idx)
    }

    /// Deregisters a module.
    fn deregister_module(
        &self,
        storage: &mut dyn Storage,
        idx: Uint64,
    ) -> Result<(), ContractError> {
        let idx_str = idx.to_string();
        self.check_module_mutability(storage, &idx_str)?;
        self.module_info.remove(storage, &idx_str);

        Ok(())
    }

    /// Alters a module
    /// If the module has provided an address as its form of instantiation this address is recorded
    /// Each module is assigned a u64 index so as it can be unregistered/altered
    /// The assigned u64 index is used as the message id for use in the `reply` entry point of the contract
    fn alter_module(
        &self,
        storage: &mut dyn Storage,
        idx: Uint64,
        module: &Module,
    ) -> Result<(), ContractError> {
        let idx_str = idx.to_string();
        self.check_module_mutability(storage, &idx_str)?;
        self.module_info.save(storage, &idx_str, module)?;
        Ok(())
    }

    fn check_module_mutability(
        &self,
        storage: &dyn Storage,
        idx_str: &str,
    ) -> Result<(), ContractError> {
        let existing_module = self.module_info.may_load(storage, idx_str)?;
        match existing_module {
            None => return Err(ContractError::ModuleDoesNotExist {}),
            Some(m) => {
                if !m.is_mutable {
                    return Err(ContractError::ModuleImmutable {});
                }
            }
        }
        Ok(())
    }

    /// Loads all registered modules in Vector form
    pub(crate) fn load_modules(&self, storage: &dyn Storage) -> Result<Vec<Module>, ContractError> {
        let module_idx = self.module_idx.may_load(storage)?.unwrap_or(1);
        let min = Some(Bound::inclusive("1"));
        let modules: Vec<Module> = self
            .module_info
            .range(storage, min, None, Order::Ascending)
            .take(module_idx.try_into().unwrap())
            .flatten()
            .map(|(_vec, module)| module)
            .collect();

        Ok(modules)
    }

    /// Loads all registered module addresses in Vector form
    fn load_module_addresses(
        &self,
        storage: &dyn Storage,
        api: &dyn Api,
        querier: &QuerierWrapper,
    ) -> Result<Vec<String>, ContractError> {
        let app_contract = self.get_app_contract(storage)?;
        let module_addresses: Result<Vec<String>, _> = self
            .load_modules(storage)?
            .iter()
            .map(|m| m.address.get_address(api, querier, app_contract.clone()))
            .collect();

        module_addresses
    }

    /// Validates all modules.
    fn validate_modules(&self, modules: &[Module], ado_type: &str) -> Result<(), ContractError> {
        for module in modules {
            module.validate(modules, ado_type)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_querier::{mock_dependencies_custom, MOCK_APP_CONTRACT};
    use common::{
        ado_base::modules::{ADDRESS_LIST, AUCTION, RECEIPT},
        app::AndrAddress,
    };
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_info},
        Addr,
    };

    #[test]
    fn test_execute_register_module_unauthorized() {
        let mut deps = mock_dependencies();

        let module = Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: false,
        };
        let deps_mut = deps.as_mut();
        ADOContract::default()
            .owner
            .save(deps_mut.storage, &Addr::unchecked("owner"))
            .unwrap();
        ADOContract::default()
            .ado_type
            .save(deps_mut.storage, &"cw20".to_string())
            .unwrap();

        let res = ADOContract::default().execute_register_module(
            deps_mut.storage,
            "sender",
            module,
            true,
        );

        assert_eq!(ContractError::Unauthorized {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_register_module_addr() {
        let mut deps = mock_dependencies();

        let module = Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: false,
        };
        let deps_mut = deps.as_mut();
        ADOContract::default()
            .owner
            .save(deps_mut.storage, &Addr::unchecked("owner"))
            .unwrap();

        ADOContract::default()
            .ado_type
            .save(deps_mut.storage, &"cw20".to_string())
            .unwrap();

        let res = ADOContract::default()
            .execute_register_module(deps_mut.storage, "owner", module.clone(), true)
            .unwrap();

        assert_eq!(
            Response::default()
                .add_attribute("action", "register_module")
                .add_attribute("module_idx", "1"),
            res
        );

        assert_eq!(
            module,
            ADOContract::default()
                .module_info
                .load(deps.as_mut().storage, "1")
                .unwrap()
        );
    }

    #[test]
    fn test_execute_register_module_validate() {
        let mut deps = mock_dependencies();

        let module = Module {
            module_type: AUCTION.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: false,
        };
        let deps_mut = deps.as_mut();
        ADOContract::default()
            .owner
            .save(deps_mut.storage, &Addr::unchecked("owner"))
            .unwrap();

        ADOContract::default()
            .ado_type
            .save(deps_mut.storage, &"cw20".to_string())
            .unwrap();

        let res = ADOContract::default().execute_register_module(
            deps_mut.storage,
            "owner",
            module.clone(),
            true,
        );

        assert_eq!(
            ContractError::IncompatibleModules {
                msg: "An Auction module cannot be used for a CW20 ADO".to_string()
            },
            res.unwrap_err(),
        );

        let _res = ADOContract::default()
            .execute_register_module(deps_mut.storage, "owner", module, false)
            .unwrap();
    }

    #[test]
    fn test_execute_alter_module_unauthorized() {
        let mut deps = mock_dependencies();
        let info = mock_info("sender", &[]);
        let module = Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: true,
        };
        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        ADOContract::default()
            .ado_type
            .save(deps.as_mut().storage, &"cw20".to_string())
            .unwrap();

        let res =
            ADOContract::default().execute_alter_module(deps.as_mut(), info, 1u64.into(), module);

        assert_eq!(ContractError::Unauthorized {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_alter_module_addr() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &[]);
        let module = Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: true,
        };

        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        ADOContract::default()
            .module_info
            .save(deps.as_mut().storage, "1", &module)
            .unwrap();
        ADOContract::default()
            .ado_type
            .save(deps.as_mut().storage, &"cw20".to_string())
            .unwrap();

        let module = Module {
            module_type: RECEIPT.to_owned(),
            address: AndrAddress {
                identifier: "other_address".to_string(),
            },
            is_mutable: true,
        };

        let res = ADOContract::default()
            .execute_alter_module(deps.as_mut(), info, 1u64.into(), module.clone())
            .unwrap();

        assert_eq!(
            Response::default()
                .add_attribute("action", "alter_module")
                .add_attribute("module_idx", "1"),
            res
        );

        assert_eq!(
            module,
            ADOContract::default()
                .module_info
                .load(deps.as_mut().storage, "1")
                .unwrap()
        );
    }

    #[test]
    fn test_execute_alter_module_immutable() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &[]);
        let module = Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: false,
        };

        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        ADOContract::default()
            .module_info
            .save(deps.as_mut().storage, "1", &module)
            .unwrap();
        ADOContract::default()
            .ado_type
            .save(deps.as_mut().storage, &"cw20".to_string())
            .unwrap();

        let module = Module {
            module_type: RECEIPT.to_owned(),
            address: AndrAddress {
                identifier: "other_address".to_string(),
            },
            is_mutable: true,
        };

        let res =
            ADOContract::default().execute_alter_module(deps.as_mut(), info, 1u64.into(), module);

        assert_eq!(ContractError::ModuleImmutable {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_alter_module_nonexisting_module() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &[]);
        let module = Module {
            module_type: AUCTION.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: true,
        };

        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();
        ADOContract::default()
            .ado_type
            .save(deps.as_mut().storage, &"cw20".to_string())
            .unwrap();

        let res =
            ADOContract::default().execute_alter_module(deps.as_mut(), info, 1u64.into(), module);

        assert_eq!(ContractError::ModuleDoesNotExist {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_alter_module_incompatible_module() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &[]);
        let module = Module {
            module_type: AUCTION.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: true,
        };

        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        ADOContract::default()
            .module_info
            .save(deps.as_mut().storage, "1", &module)
            .unwrap();
        ADOContract::default()
            .ado_type
            .save(deps.as_mut().storage, &"cw20".to_string())
            .unwrap();

        let res =
            ADOContract::default().execute_alter_module(deps.as_mut(), info, 1u64.into(), module);

        assert_eq!(
            ContractError::IncompatibleModules {
                msg: "An Auction module cannot be used for a CW20 ADO".to_string()
            },
            res.unwrap_err(),
        );
    }

    #[test]
    fn test_execute_deregister_module_unauthorized() {
        let mut deps = mock_dependencies();
        let info = mock_info("sender", &[]);
        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        let res =
            ADOContract::default().execute_deregister_module(deps.as_mut(), info, 1u64.into());

        assert_eq!(ContractError::Unauthorized {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_deregister_module() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &[]);
        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        let module = Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: true,
        };

        ADOContract::default()
            .module_info
            .save(deps.as_mut().storage, "1", &module)
            .unwrap();

        let res = ADOContract::default()
            .execute_deregister_module(deps.as_mut(), info, 1u64.into())
            .unwrap();

        assert_eq!(
            Response::default()
                .add_attribute("action", "deregister_module")
                .add_attribute("module_idx", "1"),
            res
        );

        assert!(!ADOContract::default()
            .module_info
            .has(deps.as_mut().storage, "1"));
    }

    #[test]
    fn test_execute_deregister_module_immutable() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &[]);
        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        let module = Module {
            module_type: ADDRESS_LIST.to_owned(),
            address: AndrAddress {
                identifier: "address".to_string(),
            },
            is_mutable: false,
        };

        ADOContract::default()
            .module_info
            .save(deps.as_mut().storage, "1", &module)
            .unwrap();

        let res =
            ADOContract::default().execute_deregister_module(deps.as_mut(), info, 1u64.into());
        assert_eq!(ContractError::ModuleImmutable {}, res.unwrap_err());
    }

    #[test]
    fn test_execute_deregister_module_nonexisting_module() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &[]);
        ADOContract::default()
            .owner
            .save(deps.as_mut().storage, &Addr::unchecked("owner"))
            .unwrap();

        let res =
            ADOContract::default().execute_deregister_module(deps.as_mut(), info, 1u64.into());

        assert_eq!(ContractError::ModuleDoesNotExist {}, res.unwrap_err());
    }

    #[test]
    fn test_load_module_addresses() {
        let mut deps = mock_dependencies_custom(&[]);
        let contract = ADOContract::default();
        contract
            .app_contract
            .save(deps.as_mut().storage, &Addr::unchecked(MOCK_APP_CONTRACT))
            .unwrap();
        contract.module_idx.save(deps.as_mut().storage, &2).unwrap();
        contract
            .module_info
            .save(
                deps.as_mut().storage,
                "1",
                &Module {
                    module_type: "address_list".to_string(),
                    address: AndrAddress {
                        identifier: "address".to_string(),
                    },
                    is_mutable: true,
                },
            )
            .unwrap();

        contract
            .module_info
            .save(
                deps.as_mut().storage,
                "2",
                &Module {
                    module_type: "address_list".to_string(),
                    address: AndrAddress {
                        identifier: "a".to_string(),
                    },
                    is_mutable: true,
                },
            )
            .unwrap();
        let deps_mut = deps.as_mut();
        let module_addresses = contract
            .load_module_addresses(deps_mut.storage, deps_mut.api, &deps_mut.querier)
            .unwrap();

        assert_eq!(
            vec![String::from("address"), String::from("actual_address")],
            module_addresses
        );
    }
}
