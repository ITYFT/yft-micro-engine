use std::collections::{HashMap, HashSet};

use crate::positions::position::MicroEnginePosition;

#[derive(Default, Clone, Debug)]
pub struct PositionsCacheIndex {
    pub trader_id_index: HashMap<String, HashSet<String>>,
    pub account_id_index: HashMap<String, HashSet<String>>,
    pub asset_pair_index: HashMap<String, HashSet<String>>,
    pub profit_price_subscription_indexes: HashMap<String, HashSet<String>>,
}

impl PositionsCacheIndex {
    pub fn add_index(&mut self, position: &MicroEnginePosition) {
        self.trader_id_index
            .entry(position.trader_id.clone())
            .or_default()
            .insert(position.id.clone());

        self.account_id_index
            .entry(position.account_id.clone())
            .or_default()
            .insert(position.id.clone());

        self.asset_pair_index
            .entry(position.asset_pair.clone())
            .or_default()
            .insert(position.id.clone());

        for profit_asset in &position.profit_price_assets_subscriptions {
            self.profit_price_subscription_indexes
                .entry(profit_asset.clone())
                .or_default()
                .insert(position.id.clone());
        }
    }

    pub fn remove_indexes(&mut self, position: &MicroEnginePosition) {
        Self::remove_from_index(&mut self.trader_id_index, &position.trader_id, &position.id);
        Self::remove_from_index(
            &mut self.account_id_index,
            &position.account_id,
            &position.id,
        );
        Self::remove_from_index(
            &mut self.asset_pair_index,
            &position.asset_pair,
            &position.id,
        );

        for profit_asset in &position.profit_price_assets_subscriptions {
            Self::remove_from_index(
                &mut self.profit_price_subscription_indexes,
                &profit_asset,
                &position.id,
            );
        }
    }

    fn remove_from_index(index: &mut HashMap<String, HashSet<String>>, key: &str, id: &str) {
        if let Some(set) = index.get_mut(key) {
            set.remove(id);
            if set.is_empty() {
                index.remove(key);
            }
        }
    }
}
#[cfg(test)]
mod profit_subscription_tests {

    use crate::bidask::dto::MicroEngineBidask;

    use super::*;

    fn dummy_bidask() -> MicroEngineBidask {
        MicroEngineBidask {
            id: "EURUSD".to_string().into(),
            bid: 1.1,
            ask: 1.2,
            base: "1.2".to_string().into(),
            quote: "1.2".to_string().into(),
        }
    }

    fn position_with_subscriptions(id: &str, subscriptions: &[&str]) -> MicroEnginePosition {
        let subs: HashSet<String> = subscriptions.iter().map(|s| s.to_string()).collect();

        MicroEnginePosition {
            id: id.to_string(),
            trader_id: "trader-x".to_string(),
            account_id: "acc-x".to_string(),
            asset_pair: "XAUUSD".to_string(),
            collateral: "USD".to_string(),
            lots_amount: 1.0,
            is_buy: true,
            open_bidask: dummy_bidask(),
            margin_bidask: dummy_bidask(),
            profit_bidask: dummy_bidask(),
            profit_price_assets_subscriptions: subs,
            active_bidask: dummy_bidask(),
            base: "XAU".to_string(),
            quote: "USD".to_string(),
            contract_size: 1.0,
            pl: 0.0,
            commission: 0.0,
            swaps_sum: 0.0
        }
    }

    #[test]
    fn test_add_profit_price_subscriptions() {
        let mut index = PositionsCacheIndex {
            trader_id_index: HashMap::new(),
            account_id_index: HashMap::new(),
            asset_pair_index: HashMap::new(),
            profit_price_subscription_indexes: HashMap::new(),
        };

        let position = position_with_subscriptions("pos1", &["BTCUSD", "ETHUSD"]);
        index.add_index(&position);

        for asset in &position.profit_price_assets_subscriptions {
            assert!(
                index
                    .profit_price_subscription_indexes
                    .get(asset)
                    .unwrap()
                    .contains("pos1"),
                "Missing pos1 in profit_price_subscription_indexes for {}",
                asset
            );
        }
    }

    #[test]
    fn test_remove_profit_price_subscriptions() {
        let mut index = PositionsCacheIndex {
            trader_id_index: HashMap::new(),
            account_id_index: HashMap::new(),
            asset_pair_index: HashMap::new(),
            profit_price_subscription_indexes: HashMap::new(),
        };

        let position = position_with_subscriptions("pos2", &["BTCUSD", "ETHUSD"]);
        index.add_index(&position);
        index.remove_indexes(&position);

        for asset in &position.profit_price_assets_subscriptions {
            assert!(
                index.profit_price_subscription_indexes.get(asset).is_none(),
                "Expected {} to be removed from profit_price_subscription_indexes",
                asset
            );
        }
    }
}
