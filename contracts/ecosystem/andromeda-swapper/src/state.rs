use cw_storage_plus::Item;

use common::app::AndrAddress;

pub const SWAPPER_IMPL_ADDR: Item<AndrAddress> = Item::new("swapper_impl_addr");
