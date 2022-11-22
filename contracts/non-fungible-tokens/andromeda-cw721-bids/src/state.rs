use andromeda_non_fungible_tokens::{
    cw721::{QueryMsg as CW721QueryMsg, TransferAgreement},
    cw721_bid::Bid,
};
use common::error::ContractError;
use cosmwasm_std::{QuerierWrapper, Storage};
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, MultiIndex};
use serde::{de::DeserializeOwned, Serialize};

pub const CW721_CONTRACT: Item<String> = Item::new("cw721_contract");
pub const VALID_DENOM: Item<String> = Item::new("valid_denom");

pub struct BidIndexes<'a> {
    /// (purchaser, token_id))
    pub purchaser: MultiIndex<'a, String, Bid, String>,
}

impl<'a> IndexList<Bid> for BidIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Bid>> + '_> {
        let v: Vec<&dyn Index<Bid>> = vec![&self.purchaser];
        Box::new(v.into_iter())
    }
}

pub fn bids<'a>() -> IndexedMap<'a, &'a str, Bid, BidIndexes<'a>> {
    let indexes = BidIndexes {
        purchaser: MultiIndex::new(|e| e.purchaser.clone(), "ownership", "bid_purchaser"),
    };
    IndexedMap::new("ownership", indexes)
}

pub fn query_cw721<T, M>(
    querier: QuerierWrapper,
    storage: &dyn Storage,
    msg: &M,
) -> Result<T, ContractError>
where
    T: DeserializeOwned,
    M: Serialize,
{
    let cw721_addr = CW721_CONTRACT.load(storage)?;
    let result: T = querier.query_wasm_smart(cw721_addr, &msg)?;

    Ok(result)
}

pub fn query_transfer_agreement(
    querier: QuerierWrapper,
    storage: &dyn Storage,
    token_id: String,
) -> Result<Option<TransferAgreement>, ContractError> {
    let msg = CW721QueryMsg::TransferAgreement { token_id };
    let agreement: Option<TransferAgreement> = query_cw721(querier, storage, &msg)?;

    Ok(agreement)
}

pub fn query_is_archived(
    querier: QuerierWrapper,
    storage: &dyn Storage,
    token_id: String,
) -> Result<bool, ContractError> {
    let msg = CW721QueryMsg::IsArchived { token_id };
    let is_archived: bool = query_cw721(querier, storage, &msg)?;

    Ok(is_archived)
}
