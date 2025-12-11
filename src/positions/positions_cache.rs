use std::collections::HashMap;

use crate::{
    bidask::MicroEngineBidAskCache,
    positions::{position::MicroEnginePosition, positions_cache_index::PositionsCacheIndex},
    settings::TradingSettingsCache,
};

#[derive(Debug, Clone)]
pub struct MicroEnginePositionCalculationUpdate {
    pub account_id: String,
    pub position_id: String,
    pub gross_pl: f64,
}

#[derive(Debug, Clone)]
pub struct MicroEnginePositionCache {
    indexes: PositionsCacheIndex,
    positions: HashMap<String, MicroEnginePosition>,
}

impl MicroEnginePositionCache {
    pub(crate) fn new(
        bidask_cache: &MicroEngineBidAskCache,
        positions: Vec<impl Into<MicroEnginePosition>>,
    ) -> Self {
        let mut indexes = PositionsCacheIndex::default();
        let mut positions_cache = HashMap::new();

        for position in positions {
            let mut position: MicroEnginePosition = position.into();

            if position.quote != position.collateral {
                if let Some((_, sources)) =
                    bidask_cache.get_price_with_source(&position.quote, &position.collateral)
                {
                    position.profit_price_assets_subscriptions = sources.unwrap_or_default();
                }
            }

            // Note: We don't apply markup to open_bidask here because we don't have settings yet
            // It will be applied in recalculate_all_positions or insert_or_update_position

            indexes.add_index(&position);
            positions_cache.insert(position.id.clone(), position);
        }

        Self {
            indexes,
            positions: positions_cache,
        }
    }

    pub fn get_position(&self, id: &str) -> Option<&MicroEnginePosition> {
        self.positions.get(id)
    }

    pub fn get_account_positions(&self, account_id: &str) -> Option<Vec<&MicroEnginePosition>> {
        let ids = self.indexes.account_id_index.get(account_id)?;

        let result = ids
            .into_iter()
            .filter_map(|x| self.positions.get(x))
            .collect::<Vec<_>>();

        Some(result)
    }

    pub fn get_trader_positions(&self, trader_id: &str) -> Option<Vec<&MicroEnginePosition>> {
        let ids = self.indexes.trader_id_index.get(trader_id)?;

        let result = ids
            .into_iter()
            .filter_map(|x| self.positions.get(x))
            .collect::<Vec<_>>();

        Some(result)
    }

    pub fn get_all_positions(&self) -> Vec<&MicroEnginePosition> {
        self.positions.values().collect()
    }

    pub fn add_position(&mut self, position: impl Into<MicroEnginePosition>) {
        let position: MicroEnginePosition = position.into();

        self.indexes.add_index(&position);
        self.positions.insert(position.id.clone(), position);
    }

    pub fn remove_position(&mut self, id: &str) -> Option<MicroEnginePosition> {
        let removed_position = self.positions.remove(id)?;
        self.indexes.remove_indexes(&removed_position);

        Some(removed_position)
    }

    pub fn recalculate_positions_pl(
        &mut self,
        updated_prices: &[String],
        bidask_cache: &mut MicroEngineBidAskCache,
        settings_cache: &TradingSettingsCache,
    ) -> Option<Vec<MicroEnginePositionCalculationUpdate>> {
        if updated_prices.is_empty() {
            return None;
        }

        let mut updated_positions: Option<Vec<MicroEnginePositionCalculationUpdate>> = None;
        for price_id in updated_prices.into_iter() {
            
            let target_price = match bidask_cache.get_by_id(&price_id).cloned() {
                Some(price) => price,
                None => continue,
            };

            bidask_cache.handle_new(&target_price);

            let mut positions = vec![];

            if let Some(direct_positions) = self.indexes.asset_pair_index.get(price_id) {
                positions.extend(direct_positions);
            }

            if let Some(profit_positions) =
                self.indexes.profit_price_subscription_indexes.get(price_id)
            {
                positions.extend(profit_positions);
            }

            for position_id in positions {
                if let Some(position) = self.positions.get_mut(position_id) {
                    let Some(group_settings) =
                        settings_cache.resolve_by_account(&position.account_id)
                    else {
                        continue;
                    };

                    position.update_bidask(&target_price, bidask_cache, group_settings);

                    // Explicitly update profit_bidask from raw cache prices for positions that need currency conversion
                    // This is necessary because positions loaded from trading-engine have empty profit_price_assets_subscriptions,
                    // so profit_bidask doesn't get updated in update_bidask. We need to ensure it's updated from raw cache prices
                    // to match trading-engine behavior (no markup on profit_bidask updates).
                    if position.quote != position.collateral {
                        position.update_profit_bidask_from_cache(bidask_cache);
                        position.recalculate_pl(group_settings);
                    }

                    updated_positions.get_or_insert_default().push(
                        MicroEnginePositionCalculationUpdate {
                            account_id: position.account_id.clone(),
                            position_id: position.id.clone(),
                            gross_pl: position.get_gross_pl(),
                        },
                    );
                }
            }
        }
        updated_positions
    }

    pub fn recalculate_all_positions(
        &mut self,
        bidask_cache: &mut MicroEngineBidAskCache,
        settings_cache: &TradingSettingsCache,
    ) -> Option<Vec<MicroEnginePositionCalculationUpdate>> {
        let mut updated_positions: Option<Vec<MicroEnginePositionCalculationUpdate>> = None;

        for (id, position) in self.positions.iter_mut() {
            let Some(group_settings) = settings_cache.resolve_by_account(&position.account_id)
            else {
                continue;
            };

            // Get price by asset_pair, not position ID
            // Note: We don't apply markup to open_bidask here because positions from trading engine
            // already have markup applied to open_bidask. We only apply markup to active_bidask
            // when prices update.
            if let Some(price) = bidask_cache.get_by_id(&position.asset_pair).cloned() {
                position.update_bidask(&price, bidask_cache, group_settings);

                // Explicitly update profit_bidask from raw cache prices for positions that need currency conversion
                // This is necessary because positions loaded from trading-engine have empty profit_price_assets_subscriptions,
                // so profit_bidask doesn't get updated in update_bidask. We need to ensure it's updated from raw cache prices
                // to match trading-engine behavior (no markup on profit_bidask updates).
                if position.quote != position.collateral {
                    position.update_profit_bidask_from_cache(bidask_cache);
                    position.recalculate_pl(group_settings);
                }

                updated_positions.get_or_insert_default().push(
                    MicroEnginePositionCalculationUpdate {
                        account_id: position.account_id.clone(),
                        position_id: position.id.clone(),
                        gross_pl: position.get_gross_pl(),
                    },
                );
            }
        }

        updated_positions
    }
}
