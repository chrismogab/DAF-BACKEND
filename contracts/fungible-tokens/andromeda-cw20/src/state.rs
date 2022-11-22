use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

use cw20::{AllowanceResponse};
  
#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub struct GetTokenInfo {
    pub name: String,
    pub symbol: String,
    pub owner: String
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct MinterData {
    pub minter: Addr,
    /// cap is how many more tokens can be issued by the minter
    pub cap: Option<Uint128>,
}

// impl GetTokenInfo {
//     pub fn get_cap(&self) -> Option<Uint128> {
//         self.mint.as_ref().and_then(|v| v.cap)
//     }
// }


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TokenPrice {
    pub price: Uint128,
}

pub const GET_TOKEN_INFO: Item<GetTokenInfo> = Item::new("token_info");
// pub const MARKETING_INFO: Item<MarketingInfoResponse> = Item::new("marketing_info");
// pub const LOGO: Item<Logo> = Item::new("logo");
pub const BALANCES: Map<&Addr, Uint128> = Map::new("balance");
pub const ALLOWANCES: Map<(&Addr, &Addr), AllowanceResponse> = Map::new("allowance");


pub const TOKEN_PRICE: Item<TokenPrice> = Item::new("token_price");
pub const USDC_CONTRACT: Item<String> = Item::new("usdc_contract");